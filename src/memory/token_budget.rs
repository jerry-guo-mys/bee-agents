//! Token 预算控制（解决问题 5.2）
//!
//! 为 system prompt 设置 token 预算，各记忆段按优先级竞争。

use std::collections::HashMap;

/// Token 估算器（简单的字符计数近似）
pub struct TokenEstimator;

impl TokenEstimator {
    /// 估算文本的 token 数量
    /// 使用简单的启发式规则：英文约 4 字符/token，中文约 1.5 字符/token
    pub fn estimate(text: &str) -> usize {
        let mut tokens = 0;
        let mut ascii_chars = 0;
        let mut non_ascii_chars = 0;

        for c in text.chars() {
            if c.is_ascii() {
                ascii_chars += 1;
            } else {
                non_ascii_chars += 1;
            }
        }

        // 英文按 4 字符/token，中文按 1.5 字符/token
        tokens += ascii_chars / 4;
        tokens += (non_ascii_chars as f64 / 1.5).ceil() as usize;

        tokens.max(1)
    }
}

/// 记忆段类型（按优先级排序）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemorySegment {
    /// 基础系统提示（最高优先级）
    SystemPrompt,
    /// 工具 schema
    ToolSchema,
    /// 当前目标和进度（Working Memory）
    WorkingMemory,
    /// 用户偏好
    Preferences,
    /// 行为约束/教训
    Lessons,
    /// 程序记忆（工具经验）
    Procedural,
    /// 长期记忆检索
    LongTerm,
}

impl MemorySegment {
    /// 获取优先级（数字越小优先级越高）
    pub fn priority(&self) -> u8 {
        match self {
            MemorySegment::SystemPrompt => 0,
            MemorySegment::ToolSchema => 1,
            MemorySegment::WorkingMemory => 2,
            MemorySegment::Preferences => 3,
            MemorySegment::Lessons => 4,
            MemorySegment::Procedural => 5,
            MemorySegment::LongTerm => 6,
        }
    }

    /// 所有记忆段类型
    pub fn all() -> Vec<Self> {
        vec![
            MemorySegment::SystemPrompt,
            MemorySegment::ToolSchema,
            MemorySegment::WorkingMemory,
            MemorySegment::Preferences,
            MemorySegment::Lessons,
            MemorySegment::Procedural,
            MemorySegment::LongTerm,
        ]
    }
}

/// Token 预算管理器
#[derive(Debug, Clone)]
pub struct TokenBudget {
    /// 总 token 预算
    total_budget: usize,
    /// 各段落的最大 token 数
    segment_limits: HashMap<MemorySegment, usize>,
    /// 对话历史预留 token 数
    conversation_reserve: usize,
}

impl TokenBudget {
    /// 创建新的预算管理器
    pub fn new(total_budget: usize) -> Self {
        Self {
            total_budget,
            segment_limits: HashMap::new(),
            conversation_reserve: total_budget / 3, // 预留 1/3 给对话历史
        }
    }

    /// 设置某段落的最大 token 数
    pub fn with_segment_limit(mut self, segment: MemorySegment, limit: usize) -> Self {
        self.segment_limits.insert(segment, limit);
        self
    }

    /// 设置对话历史预留 token 数
    pub fn with_conversation_reserve(mut self, reserve: usize) -> Self {
        self.conversation_reserve = reserve;
        self
    }

    /// 获取系统提示词的可用预算（总预算减去对话预留）
    pub fn system_prompt_budget(&self) -> usize {
        self.total_budget.saturating_sub(self.conversation_reserve)
    }

    /// 分配 token 给各记忆段
    /// 输入：各段落的原始内容
    /// 输出：截断后的内容（按优先级分配）
    pub fn allocate(&self, segments: &[(MemorySegment, String)]) -> Vec<(MemorySegment, String)> {
        let mut result = Vec::new();
        let mut remaining = self.system_prompt_budget();

        // 按优先级排序
        let mut sorted: Vec<_> = segments.to_vec();
        sorted.sort_by_key(|(seg, _)| seg.priority());

        for (segment, content) in sorted {
            if content.is_empty() {
                continue;
            }

            let estimated_tokens = TokenEstimator::estimate(&content);
            let segment_limit = self.segment_limits.get(&segment).copied()
                .unwrap_or(remaining);
            let allowed = remaining.min(segment_limit);

            if estimated_tokens <= allowed {
                result.push((segment, content));
                remaining = remaining.saturating_sub(estimated_tokens);
            } else if allowed > 0 {
                // 需要截断
                let truncated = Self::truncate_to_tokens(&content, allowed);
                let truncated_tokens = TokenEstimator::estimate(&truncated);
                result.push((segment, truncated));
                remaining = remaining.saturating_sub(truncated_tokens);
            }
        }

        result
    }

    /// 将文本截断到指定 token 数
    fn truncate_to_tokens(text: &str, max_tokens: usize) -> String {
        let estimated = TokenEstimator::estimate(text);
        if estimated <= max_tokens {
            return text.to_string();
        }

        // 按比例截断，保留开头部分
        let ratio = max_tokens as f64 / estimated as f64;
        let target_chars = (text.chars().count() as f64 * ratio * 0.9) as usize; // 留 10% 余量

        let truncated: String = text.chars().take(target_chars).collect();

        format!("{}...\n[truncated due to token budget]", truncated.trim_end())
    }

    /// 获取总预算
    pub fn total_budget(&self) -> usize {
        self.total_budget
    }
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self::new(8000) // 默认 8K token 预算
    }
}

/// 记忆段缓存（减少文件 I/O）
#[derive(Debug, Clone, Default)]
pub struct MemoryCache {
    /// 缓存的内容
    cache: HashMap<MemorySegment, CachedContent>,
}

#[derive(Debug, Clone)]
struct CachedContent {
    content: String,
    /// 缓存时间戳（用于判断是否过期）
    timestamp: std::time::Instant,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取缓存内容，如果过期则返回 None
    pub fn get(&self, segment: MemorySegment, max_age_secs: u64) -> Option<&str> {
        self.cache.get(&segment).and_then(|c| {
            if c.timestamp.elapsed().as_secs() < max_age_secs {
                Some(c.content.as_str())
            } else {
                None
            }
        })
    }

    /// 设置缓存内容
    pub fn set(&mut self, segment: MemorySegment, content: String) {
        self.cache.insert(segment, CachedContent {
            content,
            timestamp: std::time::Instant::now(),
        });
    }

    /// 清除指定段落的缓存
    pub fn invalidate(&mut self, segment: MemorySegment) {
        self.cache.remove(&segment);
    }

    /// 清除所有缓存
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_estimator_english() {
        let text = "Hello, world! This is a test.";
        let tokens = TokenEstimator::estimate(text);
        assert!(tokens > 0);
        assert!(tokens < text.len()); // 应该少于字符数
    }

    #[test]
    fn test_token_estimator_chinese() {
        let text = "你好世界，这是一个测试。";
        let tokens = TokenEstimator::estimate(text);
        assert!(tokens > 0);
    }

    #[test]
    fn test_token_budget_allocation() {
        let budget = TokenBudget::new(1000)
            .with_segment_limit(MemorySegment::LongTerm, 200);

        let segments = vec![
            (MemorySegment::SystemPrompt, "System prompt".to_string()),
            (MemorySegment::WorkingMemory, "Working memory".to_string()),
            (MemorySegment::LongTerm, "Long term memory content".to_string()),
        ];

        let allocated = budget.allocate(&segments);
        assert!(!allocated.is_empty());
    }

    #[test]
    fn test_memory_cache() {
        let mut cache = MemoryCache::new();
        cache.set(MemorySegment::Lessons, "Test content".to_string());

        assert!(cache.get(MemorySegment::Lessons, 60).is_some());
        assert!(cache.get(MemorySegment::Preferences, 60).is_none());

        cache.invalidate(MemorySegment::Lessons);
        assert!(cache.get(MemorySegment::Lessons, 60).is_none());
    }

    #[test]
    fn test_segment_priority() {
        assert!(MemorySegment::SystemPrompt.priority() < MemorySegment::LongTerm.priority());
        assert!(MemorySegment::WorkingMemory.priority() < MemorySegment::Procedural.priority());
    }
}
