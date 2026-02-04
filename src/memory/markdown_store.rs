//! Markdown 文件记忆存储
//!
//! - 短期：按日期日志 memory/logs/YYYY-MM-DD.md
//! - 长期：单文件 memory/long-term.md，按块检索（BM25 风格关键词 + 预留向量扩展）

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::memory::long_term::LongTermMemory;
use crate::memory::{Message, Role};

/// 记忆根目录：memory/
pub fn memory_root(workspace: &Path) -> PathBuf {
    workspace.join("memory")
}

/// 当日日志路径：memory/logs/YYYY-MM-DD.md
pub fn daily_log_path(memory_root: &Path, date: &str) -> PathBuf {
    memory_root.join("logs").join(format!("{}.md", date))
}

/// 长期记忆文件路径：memory/long-term.md
pub fn long_term_path(memory_root: &Path) -> PathBuf {
    memory_root.join("long-term.md")
}

/// 行为约束/教训文件路径：memory/lessons.md（自我进化：规则与教训，会注入 system prompt）
pub fn lessons_path(memory_root: &Path) -> PathBuf {
    memory_root.join("lessons.md")
}

/// 程序记忆文件路径：memory/procedural.md（工具成功/失败经验，会注入 system prompt）
pub fn procedural_path(memory_root: &Path) -> PathBuf {
    memory_root.join("procedural.md")
}

/// 用户偏好文件路径：memory/preferences.md（显式「记住：xxx」等，会注入 system prompt）
pub fn preferences_path(memory_root: &Path) -> PathBuf {
    memory_root.join("preferences.md")
}

/// 若存在则读取 lessons 内容，用于拼入 system prompt
pub fn load_lessons(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => s.trim().to_string(),
        Err(_) => String::new(),
    }
}

/// 若存在则读取 procedural 内容，用于拼入 system prompt（程序记忆：工具使用经验）
pub fn load_procedural(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => s.trim().to_string(),
        Err(_) => String::new(),
    }
}

/// 若存在则读取 preferences 内容，用于拼入 system prompt（用户显式偏好）
pub fn load_preferences(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => s.trim().to_string(),
        Err(_) => String::new(),
    }
}

/// 追加一条用户偏好（显式「记住：xxx」时调用）
pub fn append_preference(path: &Path, content: &str) -> std::io::Result<()> {
    if content.trim().is_empty() {
        return Ok(());
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let line = format!("- {}\n", content.trim());
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

/// 追加一条教训到 lessons.md（如 HallucinatedTool 时自动写入「仅使用以下工具：...」）
pub fn append_lesson(path: &Path, line: &str) -> std::io::Result<()> {
    if line.trim().is_empty() {
        return Ok(());
    }
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let content = format!("{}\n", line.trim());
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(content.as_bytes())
}

/// 追加一条程序记忆（工具名、成功/失败、简要原因），用于自我进化
pub fn append_procedural(path: &Path, tool: &str, success: bool, detail: &str) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let status = if success { "ok" } else { "fail" };
    let line = format!("- {} {}: {}\n", tool, status, detail);
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line.as_bytes())
}

/// 整理结果：处理了哪些日期、写入了多少条
#[derive(Debug, Default)]
pub struct ConsolidateResult {
    pub dates_processed: Vec<String>,
    pub blocks_added: usize,
}

/// 单条日志内容最大写入长期记忆的字符数（避免单日过长）
const CONSOLIDATE_MAX_CHARS_PER_DAY: usize = 6000;

/// 定期整理记忆：将近期短期日志（memory/logs/YYYY-MM-DD.md）归纳后写入长期记忆（long-term.md）
/// - since_days：整理最近几天（含今天）；例如 7 表示最近 7 天
/// - 每个日期对应一个块，标题为「整理 YYYY-MM-DD」，内容为当日日志摘要（截断以避免过长）
pub fn consolidate_memory(memory_root: &Path, since_days: u32) -> std::io::Result<ConsolidateResult> {
    let logs_dir = memory_root.join("logs");
    if !logs_dir.exists() {
        return Ok(ConsolidateResult::default());
    }
    let today = chrono::Local::now().date_naive();
    let cutoff = today - chrono::Duration::days(since_days as i64);

    let mut result = ConsolidateResult::default();
    let long_term_path = long_term_path(memory_root);
    let lt = FileLongTerm::new(long_term_path, 2000);

    let mut entries: Vec<_> = std::fs::read_dir(&logs_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().extension().map_or(false, |ext| ext == "md")
                && e.path().file_stem().and_then(|s| s.to_str()).is_some()
        })
        .collect();
    entries.sort_by_key(|e| e.path().file_name().unwrap().to_owned());

    for entry in entries {
        let path = entry.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let date = match chrono::NaiveDate::parse_from_str(stem, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue,
        };
        if date < cutoff {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let summary = summarize_log_content(&content);
        if summary.is_empty() {
            continue;
        }
        let block = format!("整理 {}：\n\n{}", stem, summary);
        lt.add(&block);
        result.dates_processed.push(stem.to_string());
        result.blocks_added += 1;
    }

    Ok(result)
}

/// 将当日日志内容做摘要：去掉 Tool call / Observation 等内部消息，保留用户与助手的实质对话，截断长度
fn summarize_log_content(content: &str) -> String {
    let mut out = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if t.is_empty() || t == "---" {
            continue;
        }
        if t.starts_with("Tool call:") || t.starts_with("Observation from ") {
            continue;
        }
        out.push(t.to_string());
    }
    let s = out.join("\n");
    if s.len() > CONSOLIDATE_MAX_CHARS_PER_DAY {
        format!("{}...", s.chars().take(CONSOLIDATE_MAX_CHARS_PER_DAY).collect::<String>())
    } else {
        s
    }
}

/// 将单轮对话追加到当日日志（短期记忆）
pub fn append_daily_log(
    memory_root: &Path,
    date: &str,
    session_id: &str,
    messages: &[Message],
) -> std::io::Result<()> {
    let path = daily_log_path(memory_root, date);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut content = String::new();
    content.push_str(&format!("\n## Session {} ({})\n\n", session_id, date));
    for m in messages {
        let (role, body) = match m.role {
            Role::User => ("User", m.content.as_str()),
            Role::Assistant => ("Assistant", m.content.as_str()),
            Role::System => ("System", m.content.as_str()),
        };
        content.push_str(&format!("### {}\n\n{}\n\n", role, body));
    }
    content.push_str("---\n\n");
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(content.as_bytes())?;
    Ok(())
}

/// 长期记忆：Markdown 文件存储 + BM25 风格关键词检索（预留向量+混合检索扩展）
#[derive(Clone)]
pub struct FileLongTerm {
    path: PathBuf,
    /// 内存缓存 (text, token_set) 用于检索；启动时从文件加载
    store: Arc<std::sync::RwLock<Vec<(String, std::collections::HashSet<String>)>>>,
    max_entries: usize,
}

/// 简单分词：按空白切分、转小写、过滤单字符，用于 BM25 检索
fn tokenize_lower(s: &str) -> std::collections::HashSet<String> {
    s.split_whitespace()
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 1)
        .collect()
}

/// 按 Markdown 二级标题（## ...）分块；无标题时整段视为一块
fn split_blocks(content: &str) -> Vec<String> {
    let content = content.trim();
    if content.is_empty() {
        return Vec::new();
    }
    let mut blocks = Vec::new();
    for block in content.split("\n\n## ") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        let text = if block.contains('\n') {
            block.splitn(2, '\n').nth(1).unwrap_or(block).trim()
        } else {
            block
        };
        if !text.is_empty() {
            blocks.push(text.to_string());
        }
    }
    if blocks.is_empty() {
        blocks.push(content.to_string());
    }
    blocks
}

impl FileLongTerm {
    pub fn new(path: PathBuf, max_entries: usize) -> Self {
        let store = Arc::new(std::sync::RwLock::new(Vec::new()));
        let mut s = Self {
            path,
            store: store.clone(),
            max_entries,
        };
        s.load_from_disk();
        s
    }

    fn load_from_disk(&mut self) {
        if !self.path.exists() {
            if let Some(p) = self.path.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            return;
        }
        if let Ok(content) = std::fs::read_to_string(&self.path) {
            let blocks = split_blocks(&content);
            let mut store = self.store.write().unwrap();
            for text in blocks {
                let tokens = tokenize_lower(&text);
                store.push((text, tokens));
            }
            let n = store.len();
            if n > self.max_entries {
                store.drain(0..n - self.max_entries);
            }
        }
    }

    /// BM25 风格得分：查询与文档词重叠数 / sqrt(文档长度)，用于排序检索结果
    fn score(
        query_tokens: &std::collections::HashSet<String>,
        doc_tokens: &std::collections::HashSet<String>,
        doc_len: usize,
    ) -> f64 {
        let overlap = query_tokens.intersection(doc_tokens).count();
        if overlap == 0 {
            return 0.0;
        }
        let doc_len = doc_len.max(1);
        (overlap as f64) / (doc_len as f64).sqrt()
    }
}

impl super::LongTermMemory for FileLongTerm {
    fn add(&self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let tokens = tokenize_lower(text);
        {
            let mut store = self.store.write().unwrap();
            store.push((text.to_string(), tokens));
            let n = store.len();
            if n > self.max_entries {
                store.drain(0..n - self.max_entries);
            }
        }
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
        let block = format!("\n\n## {}\n\n{}\n\n", timestamp, text);
        if let Some(p) = self.path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, block.as_bytes()));
    }

    fn search(&self, query: &str, k: usize) -> Vec<String> {
        let query_tokens = tokenize_lower(query);
        if query_tokens.is_empty() {
            return Vec::new();
        }
        let store = self.store.read().unwrap();
        let mut scored: Vec<(f64, String)> = store
            .iter()
            .map(|(text, doc_tokens)| {
                let s = Self::score(&query_tokens, doc_tokens, doc_tokens.len());
                (s, text.clone())
            })
            .filter(|(s, _)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(k).map(|(_, t)| t).collect()
    }

    fn enabled(&self) -> bool {
        true
    }
}
