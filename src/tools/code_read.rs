//! 代码读取工具 - 安全地读取项目代码文件
//!
//! 用于自主迭代时读取代码内容进行分析

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

/// 代码读取工具
pub struct CodeReadTool {
    /// 允许的根目录（通常是项目根目录）
    allowed_root: PathBuf,
    /// 最大读取行数
    max_lines: usize,
    /// 单行最大字符数
    max_line_length: usize,
}

impl CodeReadTool {
    pub fn new(allowed_root: impl AsRef<Path>) -> Self {
        Self {
            allowed_root: allowed_root.as_ref().to_path_buf(),
            max_lines: 2000,
            max_line_length: 2000,
        }
    }

    pub fn with_limits(mut self, max_lines: usize, max_line_length: usize) -> Self {
        self.max_lines = max_lines;
        self.max_line_length = max_line_length;
        self
    }

    /// 验证路径是否在允许范围内
    fn validate_path(&self, file_path: &str) -> Result<PathBuf, String> {
        let path = Path::new(file_path);
        
        // 解析为绝对路径
        let absolute_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.allowed_root.join(path)
        };

        // 规范化路径
        let canonical_path = match absolute_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                // 文件可能不存在，使用绝对路径继续
                absolute_path
            }
        };

        // 安全检查：确保在允许目录内
        let allowed_canonical = match self.allowed_root.canonicalize() {
            Ok(p) => p,
            Err(_) => self.allowed_root.clone(),
        };

        if !canonical_path.starts_with(&allowed_canonical) {
            return Err(format!(
                "Access denied: path '{}' is outside allowed root '{}'",
                file_path,
                self.allowed_root.display()
            ));
        }

        Ok(canonical_path)
    }

    /// 读取文件内容（带行号）
    fn read_file_with_lines(
        &self,
        file_path: &Path,
        offset: usize,
        limit: Option<usize>,
    ) -> Result<String, String> {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        if offset >= total_lines {
            return Ok(format!(
                "File '{}' has {} lines. Requested offset {} is beyond end.",
                file_path.display(),
                total_lines,
                offset
            ));
        }

        let end = limit.map(|l| (offset + l).min(total_lines)).unwrap_or(total_lines);
        let slice = &lines[offset..end];

        let mut result = String::new();
        result.push_str(&format!(
            "File: {} (lines {}-{} of {})\n",
            file_path.display(),
            offset + 1,
            end,
            total_lines
        ));
        result.push_str(&"-".repeat(60));
        result.push('\n');

        for (i, line) in slice.iter().enumerate() {
            let line_num = offset + i + 1;
            let truncated = if line.len() > self.max_line_length {
                format!("{}...", &line[..self.max_line_length])
            } else {
                line.to_string()
            };
            result.push_str(&format!("{:4}: {}\n", line_num, truncated));
        }

        if end < total_lines {
            result.push_str(&format!(
                "\n... ({} more lines, use offset={} to continue)\n",
                total_lines - end,
                end
            ));
        }

        Ok(result)
    }
}

#[async_trait]
impl Tool for CodeReadTool {
    fn name(&self) -> &str {
        "code_read"
    }

    fn description(&self) -> &str {
        r#"读取代码文件内容，返回带行号的文本。

参数:
- file_path: 文件路径（相对项目根目录或绝对路径）
- offset: 起始行号（从1开始，可选，默认1）
- limit: 最大读取行数（可选，默认200）

返回: 带行号的文件内容

示例:
{"file_path": "src/main.rs", "offset": 1, "limit": 50}"#
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: file_path")?;

        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(1)
            .saturating_sub(1); // 转换为0-based

        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .or(Some(200));

        let validated_path = self.validate_path(file_path)?;
        
        if !validated_path.exists() {
            return Err(format!("File not found: {}", validated_path.display()));
        }

        if !validated_path.is_file() {
            return Err(format!("Path is not a file: {}", validated_path.display()));
        }

        self.read_file_with_lines(&validated_path, offset, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path_security() {
        let test_dir = std::path::PathBuf::from("./target/test_code_read");
        std::fs::create_dir_all(&test_dir).unwrap();
        std::fs::create_dir_all(test_dir.join("src")).unwrap();
        std::fs::write(test_dir.join("Cargo.toml"), "").unwrap();
        std::fs::write(test_dir.join("src/main.rs"), "fn main() {}").unwrap();
        
        let tool = CodeReadTool::new(&test_dir);
        
        // 正常路径
        assert!(tool.validate_path("src/main.rs").is_ok());
        assert!(tool.validate_path("Cargo.toml").is_ok());
        
        // 路径穿越攻击应该被阻止
        assert!(tool.validate_path("../../../etc/passwd").is_err());
        assert!(tool.validate_path("src/../../../etc/passwd").is_err());
        
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_read_nonexistent_file() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let tool = CodeReadTool::new(".");
        
        rt.block_on(async {
            let args = serde_json::json!({
                "file_path": "nonexistent_file_xyz.txt"
            });
            let result = tool.execute(args).await;
            assert!(result.is_err());
        });
    }
}
