//! RAG (Retrieval-Augmented Generation) Pipeline
//!
//! 提供文档分块、向量化存储、检索和生成增强功能。
//! 用于增强长期记忆的检索能力和答案生成质量。

use std::collections::HashMap;
use std::sync::Arc;

use crate::llm::EmbeddingProvider;
use crate::memory::tokenizer;

/// 文档块
#[derive(Debug, Clone)]
pub struct Chunk {
    /// 块 ID
    pub id: String,
    /// 原始文本
    pub text: String,
    /// 来源文档 ID
    pub source_id: String,
    /// 在原文档中的位置（字符偏移）
    pub offset: usize,
    /// 元数据
    pub metadata: HashMap<String, String>,
}

impl Chunk {
    pub fn new(id: impl Into<String>, text: impl Into<String>, source_id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            source_id: source_id.into(),
            offset: 0,
            metadata: HashMap::new(),
        }
    }

    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

/// 分块策略
#[derive(Debug, Clone)]
pub struct ChunkingConfig {
    /// 目标块大小（字符数）
    pub chunk_size: usize,
    /// 块之间的重叠（字符数）
    pub chunk_overlap: usize,
    /// 分隔符优先级（从高到低）
    pub separators: Vec<String>,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            chunk_size: 500,
            chunk_overlap: 50,
            separators: vec![
                "\n\n".to_string(),
                "\n".to_string(),
                "。".to_string(),
                ". ".to_string(),
                "！".to_string(),
                "？".to_string(),
                "! ".to_string(),
                "? ".to_string(),
                " ".to_string(),
            ],
        }
    }
}

/// 文档分块器
pub struct Chunker {
    config: ChunkingConfig,
}

impl Chunker {
    pub fn new(config: ChunkingConfig) -> Self {
        Self { config }
    }

    /// 将文档分割为块（UTF-8 安全）
    pub fn chunk(&self, doc_id: &str, text: &str) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let total_chars = chars.len();
        
        if total_chars == 0 {
            return chunks;
        }

        let mut current_idx = 0;
        let mut chunk_idx = 0;

        while current_idx < total_chars {
            // 计算这个块的结束位置
            let target_end = (current_idx + self.config.chunk_size).min(total_chars);
            let mut actual_end = target_end;

            // 如果不是文档末尾，尝试在分隔符处断开
            if target_end < total_chars {
                let slice: String = chars[current_idx..target_end].iter().collect();
                for sep in &self.config.separators {
                    if let Some(pos) = slice.rfind(sep) {
                        let chars_to_sep: usize = slice[..pos].chars().count() + sep.chars().count();
                        if chars_to_sep > 0 {
                            actual_end = current_idx + chars_to_sep;
                            break;
                        }
                    }
                }
            }

            // 确保至少前进一个字符
            if actual_end <= current_idx {
                actual_end = (current_idx + 1).min(total_chars);
            }

            // 提取块文本
            let chunk_text: String = chars[current_idx..actual_end].iter().collect();
            let trimmed = chunk_text.trim();
            
            if !trimmed.is_empty() {
                let byte_offset: usize = chars[..current_idx].iter().map(|c| c.len_utf8()).sum();
                let chunk = Chunk::new(
                    format!("{}_{}", doc_id, chunk_idx),
                    trimmed,
                    doc_id,
                )
                .with_offset(byte_offset);
                chunks.push(chunk);
                chunk_idx += 1;
            }

            // 计算下一个块的起始位置
            let overlap = self.config.chunk_overlap.min(actual_end - current_idx);
            let next_start = actual_end.saturating_sub(overlap);
            
            // 确保前进
            current_idx = if next_start > current_idx {
                next_start
            } else {
                actual_end
            };
        }

        chunks
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new(ChunkingConfig::default())
    }
}

/// 检索结果
#[derive(Debug, Clone)]
pub struct RetrievalResult {
    /// 检索到的块
    pub chunk: Chunk,
    /// 相似度分数
    pub score: f32,
}

/// 向量存储
pub struct VectorStore {
    /// (chunk_id, chunk, embedding)
    entries: Vec<(String, Chunk, Vec<f32>)>,
    /// 嵌入提供者
    embedder: Arc<dyn EmbeddingProvider>,
    /// 最大条目数
    max_entries: usize,
}

impl VectorStore {
    pub fn new(embedder: Arc<dyn EmbeddingProvider>, max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            embedder,
            max_entries,
        }
    }

    /// 添加块到存储
    pub fn add(&mut self, chunk: Chunk) -> Result<(), String> {
        let embedding = self.embedder.embed_sync(&chunk.text)?;
        if embedding.is_empty() {
            return Err("Empty embedding".to_string());
        }
        
        let id = chunk.id.clone();
        self.entries.push((id, chunk, embedding));

        // 超出限制时移除最旧的
        if self.entries.len() > self.max_entries {
            self.entries.drain(0..self.entries.len() - self.max_entries);
        }

        Ok(())
    }

    /// 批量添加块
    pub fn add_chunks(&mut self, chunks: Vec<Chunk>) -> Result<usize, String> {
        let mut added = 0;
        for chunk in chunks {
            if self.add(chunk).is_ok() {
                added += 1;
            }
        }
        Ok(added)
    }

    /// 检索最相关的块
    pub fn search(&self, query: &str, k: usize) -> Vec<RetrievalResult> {
        let query_embedding = match self.embedder.embed_sync(query) {
            Ok(v) if !v.is_empty() => v,
            _ => return Vec::new(),
        };

        let mut scored: Vec<(f32, &Chunk)> = self
            .entries
            .iter()
            .map(|(_, chunk, emb)| (cosine_similarity(&query_embedding, emb), chunk))
            .filter(|(score, _)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        scored
            .into_iter()
            .take(k)
            .map(|(score, chunk)| RetrievalResult {
                chunk: chunk.clone(),
                score,
            })
            .collect()
    }

    /// 混合检索：结合向量检索和关键词检索
    pub fn hybrid_search(&self, query: &str, k: usize) -> Vec<RetrievalResult> {
        // 向量检索结果
        let vector_results = self.search(query, k * 2);
        
        // 关键词检索
        let query_tokens = tokenizer::tokenize_to_set(query);
        let mut keyword_scored: Vec<(f32, &Chunk)> = self
            .entries
            .iter()
            .map(|(_, chunk, _)| {
                let chunk_tokens = tokenizer::tokenize_to_set(&chunk.text);
                let score = tokenizer::jaccard_similarity(&query_tokens, &chunk_tokens);
                (score, chunk)
            })
            .filter(|(score, _)| *score > 0.0)
            .collect();
        
        keyword_scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        
        // 合并结果（RRF - Reciprocal Rank Fusion）
        let mut scores: HashMap<String, f32> = HashMap::new();
        let rrf_k = 60.0; // RRF 常数
        
        for (rank, result) in vector_results.iter().enumerate() {
            let rrf_score = 1.0 / (rrf_k + rank as f32);
            *scores.entry(result.chunk.id.clone()).or_insert(0.0) += rrf_score;
        }
        
        for (rank, (_, chunk)) in keyword_scored.iter().take(k * 2).enumerate() {
            let rrf_score = 1.0 / (rrf_k + rank as f32);
            *scores.entry(chunk.id.clone()).or_insert(0.0) += rrf_score;
        }
        
        // 按融合分数排序
        let mut final_results: Vec<RetrievalResult> = scores
            .into_iter()
            .filter_map(|(id, score)| {
                self.entries
                    .iter()
                    .find(|(entry_id, _, _)| entry_id == &id)
                    .map(|(_, chunk, _)| RetrievalResult {
                        chunk: chunk.clone(),
                        score,
                    })
            })
            .collect();
        
        final_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        final_results.truncate(k);
        
        final_results
    }

    /// 删除指定来源的所有块
    pub fn remove_by_source(&mut self, source_id: &str) {
        self.entries.retain(|(_, chunk, _)| chunk.source_id != source_id);
    }

    /// 获取存储条目数
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 存储是否为空
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 余弦相似度
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// RAG Pipeline
pub struct RagPipeline {
    chunker: Chunker,
    vector_store: VectorStore,
}

impl RagPipeline {
    pub fn new(embedder: Arc<dyn EmbeddingProvider>, max_entries: usize) -> Self {
        Self {
            chunker: Chunker::default(),
            vector_store: VectorStore::new(embedder, max_entries),
        }
    }

    pub fn with_chunking_config(mut self, config: ChunkingConfig) -> Self {
        self.chunker = Chunker::new(config);
        self
    }

    /// 索引文档
    pub fn index_document(&mut self, doc_id: &str, text: &str) -> Result<usize, String> {
        // 先删除旧版本
        self.vector_store.remove_by_source(doc_id);
        
        // 分块并索引
        let chunks = self.chunker.chunk(doc_id, text);
        self.vector_store.add_chunks(chunks)
    }

    /// 检索相关上下文
    pub fn retrieve(&self, query: &str, k: usize) -> Vec<RetrievalResult> {
        self.vector_store.hybrid_search(query, k)
    }

    /// 构建增强提示（将检索结果整合到提示中）
    pub fn build_augmented_prompt(&self, query: &str, k: usize) -> String {
        let results = self.retrieve(query, k);
        
        if results.is_empty() {
            return query.to_string();
        }

        let mut context = String::from("根据以下相关上下文回答问题:\n\n");
        for (i, result) in results.iter().enumerate() {
            context.push_str(&format!(
                "[Context {}] (相关度: {:.2})\n{}\n\n",
                i + 1,
                result.score,
                result.chunk.text
            ));
        }
        context.push_str(&format!("问题: {}", query));
        
        context
    }

    /// 获取存储统计
    pub fn stats(&self) -> (usize, usize) {
        (self.vector_store.len(), 0) // (entries, documents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunking() {
        let chunker = Chunker::new(ChunkingConfig {
            chunk_size: 100,
            chunk_overlap: 20,
            ..Default::default()
        });

        let text = "这是第一段话。这是第二句话。这是第三句话。\n\n这是第二段。这里有更多内容。";
        let chunks = chunker.chunk("doc1", text);

        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert!(!chunk.text.is_empty());
            assert_eq!(chunk.source_id, "doc1");
        }
    }

    #[test]
    fn test_chunk_metadata() {
        let chunk = Chunk::new("id1", "text", "source1")
            .with_offset(100)
            .with_metadata("type", "paragraph");

        assert_eq!(chunk.id, "id1");
        assert_eq!(chunk.offset, 100);
        assert_eq!(chunk.metadata.get("type"), Some(&"paragraph".to_string()));
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &c)).abs() < 0.001);
    }

    #[test]
    fn test_chunking_config_default() {
        let config = ChunkingConfig::default();
        assert_eq!(config.chunk_size, 500);
        assert_eq!(config.chunk_overlap, 50);
        assert!(!config.separators.is_empty());
    }
}
