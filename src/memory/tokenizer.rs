//! 分词模块（解决问题 5.1）
//!
//! 提供中英文混合分词能力，用于长期记忆检索。
//! 使用 jieba-rs 进行中文分词，英文按空格分词。

use std::collections::HashSet;
use std::sync::OnceLock;

use jieba_rs::Jieba;

/// 全局 Jieba 实例（延迟初始化）
static JIEBA: OnceLock<Jieba> = OnceLock::new();

fn get_jieba() -> &'static Jieba {
    JIEBA.get_or_init(Jieba::new)
}

/// 判断字符是否为 CJK（中日韩）字符
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{4E00}'..='\u{9FFF}' |   // CJK Unified Ideographs
        '\u{3400}'..='\u{4DBF}' |   // CJK Unified Ideographs Extension A
        '\u{F900}'..='\u{FAFF}' |   // CJK Compatibility Ideographs
        '\u{3000}'..='\u{303F}' |   // CJK Symbols and Punctuation
        '\u{3040}'..='\u{309F}' |   // Hiragana
        '\u{30A0}'..='\u{30FF}'     // Katakana
    )
}

/// 判断文本是否包含 CJK 字符
pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(is_cjk)
}

/// 智能分词：根据文本内容自动选择分词策略
/// - 包含 CJK 字符时使用 jieba 分词
/// - 纯英文时使用空格分词
pub fn tokenize(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    if contains_cjk(text) {
        // 使用 jieba 进行中文分词（搜索引擎模式，更细粒度）
        get_jieba()
            .cut_for_search(text, true)
            .into_iter()
            .map(|s| s.to_lowercase())
            .filter(|s| s.len() > 1 || is_cjk(s.chars().next().unwrap_or(' ')))
            .collect()
    } else {
        // 纯英文：按空格分词
        text.split_whitespace()
            .map(|s| s.to_lowercase())
            .filter(|s| s.len() > 1)
            .collect()
    }
}

/// 分词并返回词集合（用于相似度计算）
pub fn tokenize_to_set(text: &str) -> HashSet<String> {
    tokenize(text).into_iter().collect()
}

/// 计算两个词集合的相似度（Jaccard 相似度）
pub fn jaccard_similarity(set1: &HashSet<String>, set2: &HashSet<String>) -> f32 {
    if set1.is_empty() || set2.is_empty() {
        return 0.0;
    }
    let intersection = set1.intersection(set2).count() as f32;
    let union = set1.union(set2).count() as f32;
    intersection / union
}

/// 计算两个词集合的重叠分数（交集大小）
pub fn overlap_score(set1: &HashSet<String>, set2: &HashSet<String>) -> usize {
    set1.intersection(set2).count()
}

/// 带 TF-IDF 权重的相似度计算（简化版）
/// 假设较长的词更重要
pub fn weighted_similarity(tokens1: &[String], tokens2: &[String]) -> f32 {
    if tokens1.is_empty() || tokens2.is_empty() {
        return 0.0;
    }
    
    let set1: HashSet<_> = tokens1.iter().collect();
    let set2: HashSet<_> = tokens2.iter().collect();
    
    let mut score = 0.0;
    for token in set1.intersection(&set2) {
        // 较长的词权重更高
        score += (token.chars().count() as f32).sqrt();
    }
    
    let max_possible = tokens1.iter().chain(tokens2.iter())
        .map(|t| (t.chars().count() as f32).sqrt())
        .sum::<f32>();
    
    if max_possible > 0.0 {
        score / max_possible * 2.0 // 乘2是因为相同词在两边各算一次
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_chinese() {
        let tokens = tokenize("我喜欢编程和人工智能");
        assert!(!tokens.is_empty());
        // jieba 会将中文正确分词
        assert!(tokens.iter().any(|t| t.contains("编程") || t.contains("人工") || t.contains("智能")));
    }

    #[test]
    fn test_tokenize_english() {
        let tokens = tokenize("I like programming and AI");
        assert!(!tokens.is_empty());
        assert!(tokens.contains(&"programming".to_string()));
        assert!(tokens.contains(&"like".to_string()));
    }

    #[test]
    fn test_tokenize_mixed() {
        let tokens = tokenize("我喜欢 Rust 编程语言");
        assert!(!tokens.is_empty());
        // 应该同时包含中文和英文分词
        assert!(tokens.iter().any(|t| t == "rust" || t.contains("编程")));
    }

    #[test]
    fn test_contains_cjk() {
        assert!(contains_cjk("你好"));
        assert!(contains_cjk("Hello 世界"));
        assert!(!contains_cjk("Hello World"));
    }

    #[test]
    fn test_jaccard_similarity() {
        let set1 = tokenize_to_set("我喜欢编程");
        let set2 = tokenize_to_set("我也喜欢编程");
        let sim = jaccard_similarity(&set1, &set2);
        assert!(sim > 0.0, "Similar texts should have positive similarity");
    }

    #[test]
    fn test_overlap_score() {
        let set1 = tokenize_to_set("Rust programming");
        let set2 = tokenize_to_set("Rust language");
        let score = overlap_score(&set1, &set2);
        assert!(score >= 1, "Should have at least 'rust' in common");
    }

    #[test]
    fn test_weighted_similarity() {
        let tokens1 = tokenize("人工智能是未来的趋势");
        let tokens2 = tokenize("人工智能改变世界");
        let sim = weighted_similarity(&tokens1, &tokens2);
        assert!(sim > 0.0, "Similar texts should have positive weighted similarity");
    }
}
