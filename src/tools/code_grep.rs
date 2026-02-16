//! 代码搜索工具 - 在代码库中搜索模式
//!
//! 用于自主迭代时查找代码位置

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

/// 代码搜索工具
pub struct CodeGrepTool {
    allowed_root: PathBuf,
    max_results: usize,
    max_file_size: usize,
}

impl CodeGrepTool {
    pub fn new(allowed_root: impl AsRef<Path>) -> Self {
        Self {
            allowed_root: allowed_root.as_ref().to_path_buf(),
            max_results: 50,
            max_file_size: 1024 * 1024, // 1MB
        }
    }

    pub fn with_limits(mut self, max_results: usize, max_file_size: usize) -> Self {
        self.max_results = max_results;
        self.max_file_size = max_file_size;
        self
    }

    fn validate_path(&self, file_path: &str) -> Result<PathBuf, String> {
        let path = Path::new(file_path);
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.allowed_root.join(path)
        };

        let canonical_path = match absolute_path.canonicalize() {
            Ok(p) => p,
            Err(_) => absolute_path,
        };

        let allowed_canonical = match self.allowed_root.canonicalize() {
            Ok(p) => p,
            Err(_) => self.allowed_root.clone(),
        };

        if !canonical_path.starts_with(&allowed_canonical) {
            return Err(format!(
                "Access denied: path '{}' is outside allowed root",
                file_path
            ));
        }

        Ok(canonical_path)
    }

    fn search_in_file(
        &self,
        file_path: &Path,
        pattern: &str,
        use_regex: bool,
    ) -> Result<Vec<(usize, String)>, String> {
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return Ok(vec![]), // 跳过无法读取的文件
        };

        let mut matches = Vec::new();

        if use_regex {
            match regex::Regex::new(pattern) {
                Ok(re) => {
                    for (line_num, line) in content.lines().enumerate() {
                        if re.is_match(line) {
                            matches.push((line_num + 1, line.to_string()));
                        }
                    }
                }
                Err(e) => return Err(format!("Invalid regex pattern: {}", e)),
            }
        } else {
            for (line_num, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    matches.push((line_num + 1, line.to_string()));
                }
            }
        }

        Ok(matches)
    }

    fn search_recursive(
        &self,
        dir: &Path,
        pattern: &str,
        include: Option<&str>,
        use_regex: bool,
    ) -> Result<Vec<SearchResult>, String> {
        let mut results = Vec::new();
        let include_pattern = include.map(|p| {
            glob::Pattern::new(p).unwrap_or_else(|_| glob::Pattern::new("*").unwrap())
        });

        for entry in walkdir::WalkDir::new(dir)
            .max_depth(10)
            .into_iter()
            .filter_entry(|e| {
                // 跳过隐藏目录和目标目录
                let name = e.file_name().to_string_lossy();
                !name.starts_with('.') && name != "target" && name != "node_modules"
            })
            .filter_map(|e| e.ok())
        {
            if results.len() >= self.max_results {
                break;
            }

            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();

            // 检查文件大小
            if let Ok(metadata) = entry.metadata() {
                if metadata.len() > self.max_file_size as u64 {
                    continue;
                }
            }

            // 检查文件扩展名模式
            if let Some(ref pattern) = include_pattern {
                let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !pattern.matches(file_name) {
                    continue;
                }
            }

            // 搜索文件内容
            match self.search_in_file(path, pattern, use_regex) {
                Ok(matches) if !matches.is_empty() => {
                    results.push(SearchResult {
                        file_path: path.to_path_buf(),
                        matches,
                    });
                }
                _ => {}
            }
        }

        Ok(results)
    }
}

struct SearchResult {
    file_path: PathBuf,
    matches: Vec<(usize, String)>,
}

#[async_trait]
impl Tool for CodeGrepTool {
    fn name(&self) -> &str {
        "code_grep"
    }

    fn description(&self) -> &str {
        r#"在代码库中搜索模式，支持正则表达式。

参数:
- pattern: 搜索模式（字符串或正则表达式）
- path: 搜索路径（可选，默认为项目根目录）
- include: 文件过滤模式（可选，如 "*.rs", "*.toml"）
- use_regex: 是否使用正则（可选，默认 false）

返回: 匹配的文件列表及行号

示例:
{"pattern": "fn main", "include": "*.rs", "use_regex": false}"#
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: pattern")?;

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");

        let include = args.get("include").and_then(|v| v.as_str());

        let use_regex = args
            .get("use_regex")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let search_path = self.validate_path(path)?;

        let results = if search_path.is_file() {
            // 搜索单个文件
            let matches = self.search_in_file(&search_path, pattern, use_regex)?;
            if matches.is_empty() {
                vec![]
            } else {
                vec![SearchResult {
                    file_path: search_path,
                    matches,
                }]
            }
        } else {
            // 递归搜索目录
            self.search_recursive(&search_path, pattern, include, use_regex)?
        };

        if results.is_empty() {
            return Ok(format!("No matches found for pattern '{}'", pattern));
        }

        let mut output = String::new();
        output.push_str(&format!(
            "Found {} matches for pattern '{}'\n",
            results.iter().map(|r| r.matches.len()).sum::<usize>(),
            pattern
        ));
        output.push_str(&"=".repeat(60));
        output.push('\n');

        for result in results {
            output.push_str(&format!("\n{}:\n", result.file_path.display()));
            for (line_num, line) in result.matches.iter().take(10) {
                let truncated = if line.len() > 100 {
                    format!("{}...", &line[..100])
                } else {
                    line.clone()
                };
                output.push_str(&format!("  {:4}: {}\n", line_num, truncated));
            }
            if result.matches.len() > 10 {
                output.push_str(&format!("  ... ({} more matches)\n", result.matches.len() - 10));
            }
        }

        Ok(output)
    }
}
