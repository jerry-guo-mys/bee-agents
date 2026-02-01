//! 长期记忆：向量化知识、用户偏好，跨会话检索
//!
//! 支持 add(text) 与 search(query, k)；当前实现为 InMemoryLongTerm（关键词重叠），
//! 后续可接 Qdrant/LanceDB 等真实向量库。

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
