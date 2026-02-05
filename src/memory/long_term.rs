//! 长期记忆：向量化知识、用户偏好，跨会话检索
//!
//! 支持 add(text) 与 search(query, k)。实现：FileLongTerm（BM25）、InMemoryLongTerm（词重叠）、
//! InMemoryVectorLongTerm（嵌入 API + 余弦相似度，config [memory].vector_enabled 启用；支持快照持久化）。

use std::path::Path;
use std::sync::Arc;

/// 长期记忆 trait：支持写入与相似度检索
pub trait LongTermMemory: Send + Sync {
    /// 存入一段文本（可后续按 query 检索）
    fn add(&self, text: &str);

    /// 按查询检索最相关的 k 条，返回文本片段
    fn search(&self, query: &str, k: usize) -> Vec<String>;

    /// 是否启用（Noop 实现返回 false）
    fn enabled(&self) -> bool {
        true
    }
}

/// 空实现：未启用长期记忆时使用
#[derive(Clone, Default)]
pub struct NoopLongTerm;

impl LongTermMemory for NoopLongTerm {
    fn add(&self, _text: &str) {}

    fn search(&self, _query: &str, _k: usize) -> Vec<String> {
        Vec::new()
    }

    fn enabled(&self) -> bool {
        false
    }
}

/// 简单内存实现：按关键词重叠检索（无真实向量，适合 MVP）
#[derive(Clone)]
pub struct InMemoryLongTerm {
    /// (text, 小写词集合) 用于简单匹配
    store: Arc<std::sync::RwLock<Vec<(String, std::collections::HashSet<String>)>>>,
    max_entries: usize,
}

/// 将文本切分为小写词集合，用于简单相似度（词重叠数）
fn tokenize_lower(s: &str) -> std::collections::HashSet<String> {
    s.split_whitespace()
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 1)
        .collect()
}

impl InMemoryLongTerm {
    pub fn new(max_entries: usize) -> Self {
        Self {
            store: Arc::new(std::sync::RwLock::new(Vec::new())),
            max_entries,
        }
    }

    /// 相似度：查询词与文档词的交集大小
    fn score(&self, query_tokens: &std::collections::HashSet<String>, doc_tokens: &std::collections::HashSet<String>) -> usize {
        query_tokens.intersection(doc_tokens).count()
    }
}

impl LongTermMemory for InMemoryLongTerm {
    fn add(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let tokens = tokenize_lower(text);
        let mut store = self.store.write().unwrap();
        store.push((text.to_string(), tokens));
        let n = store.len();
        if n > self.max_entries {
            store.drain(0..n - self.max_entries);
        }
    }

    fn search(&self, query: &str, k: usize) -> Vec<String> {
        let query_tokens = tokenize_lower(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }
        let store = self.store.read().unwrap();
        let mut scored: Vec<(usize, String)> = store
            .iter()
            .map(|(text, doc_tokens)| (self.score(&query_tokens, doc_tokens), text.clone()))
            .filter(|(s, _)| *s > 0)
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored
            .into_iter()
            .take(k)
            .map(|(_, t)| t)
            .collect()
    }
}

impl Default for InMemoryLongTerm {
    fn default() -> Self {
        Self::new(1000)
    }
}

/// 向量长期记忆：调用嵌入 API 将文本转为向量，检索时按余弦相似度返回 top-k；可选快照路径实现持久化
pub struct InMemoryVectorLongTerm {
    store: Arc<std::sync::RwLock<Vec<(String, Vec<f32>)>>>,
    embedder: Arc<dyn crate::llm::EmbeddingProvider>,
    max_entries: usize,
    snapshot_path: Option<std::path::PathBuf>,
}

/// 快照 JSON 条目（与 vector_snapshot.json 格式一致）
#[derive(serde::Serialize, serde::Deserialize)]
struct VectorSnapshotEntry {
    text: String,
    embedding: Vec<f32>,
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        assert_eq!(cosine_similarity(&[], &[1.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!((cosine_similarity(&[1.0, 1.0], &[1.0, 1.0]) - 1.0).abs() < 1e-6);
    }
}

impl InMemoryVectorLongTerm {
    /// 仅内存，不持久化
    pub fn new(embedder: Arc<dyn crate::llm::EmbeddingProvider>, max_entries: usize) -> Self {
        Self::new_with_persistence(embedder, max_entries, Option::<std::path::PathBuf>::None)
    }

    /// 可选快照路径：若存在则启动时加载；可调用 save_snapshot() 定期或退出时保存
    pub fn new_with_persistence(
        embedder: Arc<dyn crate::llm::EmbeddingProvider>,
        max_entries: usize,
        snapshot_path: Option<impl AsRef<Path>>,
    ) -> Self {
        let path_buf = snapshot_path.map(|p| p.as_ref().to_path_buf());
        let store = Arc::new(std::sync::RwLock::new(Vec::new()));
        if let Some(ref path) = path_buf {
            if let Ok(data) = std::fs::read_to_string(path) {
                if let Ok(entries) = serde_json::from_str::<Vec<VectorSnapshotEntry>>(&data) {
                    let loaded: Vec<(String, Vec<f32>)> =
                        entries.into_iter().map(|e| (e.text, e.embedding)).collect();
                    let n = loaded.len().min(max_entries);
                    let start = loaded.len().saturating_sub(n);
                    store.write().unwrap().extend(loaded.into_iter().skip(start));
                    tracing::info!("vector long-term loaded {} entries from snapshot", n);
                }
            }
        }
        Self {
            store,
            embedder,
            max_entries,
            snapshot_path: path_buf,
        }
    }

    /// 将当前 store 写入快照路径（若配置了 snapshot_path）
    pub fn save_snapshot(&self) {
        if let Some(ref path) = self.snapshot_path {
            let store = self.store.read().unwrap();
            let entries: Vec<VectorSnapshotEntry> = store
                .iter()
                .map(|(text, emb)| VectorSnapshotEntry {
                    text: text.clone(),
                    embedding: emb.clone(),
                })
                .collect();
            drop(store);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(&entries) {
                if std::fs::write(path, json).is_ok() {
                    tracing::debug!("vector snapshot saved to {:?}", path);
                }
            }
        }
    }
}

impl LongTermMemory for InMemoryVectorLongTerm {
    fn add(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        match self.embedder.embed_sync(text) {
            Ok(vec) if !vec.is_empty() => {
                let mut store = self.store.write().unwrap();
                store.push((text.to_string(), vec));
                let n = store.len();
                if n > self.max_entries {
                    store.drain(0..n - self.max_entries);
                }
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("vector long-term embed failed: {}", e),
        }
    }

    fn search(&self, query: &str, k: usize) -> Vec<String> {
        let query = query.trim();
        if query.is_empty() {
            return Vec::new();
        }
        let query_vec = match self.embedder.embed_sync(query) {
            Ok(v) if !v.is_empty() => v,
            _ => return Vec::new(),
        };
        let store = self.store.read().unwrap();
        let mut scored: Vec<(f32, String)> = store
            .iter()
            .map(|(text, vec)| (cosine_similarity(&query_vec, vec), text.clone()))
            .filter(|(s, _)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(k).map(|(_, t)| t).collect()
    }
}
