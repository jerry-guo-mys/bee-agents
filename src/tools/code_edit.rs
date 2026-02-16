//! 代码编辑工具 - 安全地修改代码文件
//!
//! 用于自主迭代时执行代码修改，支持精确字符串替换

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

/// 代码编辑工具
pub struct CodeEditTool {
    allowed_root: PathBuf,
    max_file_size: usize,
    backup_enabled: bool,
}

/// 编辑操作结果
#[derive(Debug)]
struct EditResult {
    success: bool,
    message: String,
    #[allow(dead_code)]
    old_string: String,
    #[allow(dead_code)]
    new_string: String,
    line_number: Option<usize>,
}

impl CodeEditTool {
    pub fn new(allowed_root: impl AsRef<Path>) -> Self {
        Self {
            allowed_root: allowed_root.as_ref().to_path_buf(),
            max_file_size: 10 * 1024 * 1024, // 10MB
            backup_enabled: true,
        }
    }

    pub fn with_backup(mut self, enabled: bool) -> Self {
        self.backup_enabled = enabled;
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

    fn create_backup(&self, file_path: &Path) -> Result<PathBuf, String> {
        if !self.backup_enabled {
            return Ok(file_path.to_path_buf());
        }

        let backup_path = file_path.with_extension("bak");
        std::fs::copy(file_path, &backup_path)
            .map_err(|e| format!("Failed to create backup: {}", e))?;
        
        Ok(backup_path)
    }

    fn find_exact_match(&self, content: &str, old_string: &str) -> Option<usize> {
        content.find(old_string)
    }

    fn find_with_indentation_tolerance(
        &self,
        content: &str,
        old_string: &str,
    ) -> Option<(usize, String)> {
        let old_lines: Vec<&str> = old_string.lines().collect();
        if old_lines.is_empty() {
            return None;
        }

        let content_lines: Vec<&str> = content.lines().collect();
        let first_line = old_lines[0].trim_start();

        for (i, line) in content_lines.iter().enumerate() {
            if line.trim_start() == first_line {
                // 检查后续行是否匹配
                let mut matched = true;
                let mut reconstructed = String::new();
                
                for (j, old_line) in old_lines.iter().enumerate() {
                    if i + j >= content_lines.len() {
                        matched = false;
                        break;
                    }
                    
                    let content_line = content_lines[i + j];
                    let old_trimmed = old_line.trim_start();
                    let content_trimmed = content_line.trim_start();
                    
                    if old_trimmed != content_trimmed {
                        matched = false;
                        break;
                    }
                    
                    if j > 0 {
                        reconstructed.push('\n');
                    }
                    reconstructed.push_str(content_line);
                }

                if matched {
                    // 计算字节位置
                    let byte_pos: usize = content_lines[..i].join("\n").len()
                        + if i > 0 { 1 } else { 0 };
                    return Some((byte_pos, reconstructed));
                }
            }
        }

        None
    }

    fn perform_edit(
        &self,
        file_path: &Path,
        old_string: &str,
        new_string: &str,
    ) -> Result<EditResult, String> {
        // 读取文件
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        if content.len() > self.max_file_size {
            return Err(format!(
                "File too large: {} bytes (max: {})",
                content.len(),
                self.max_file_size
            ));
        }

        // 创建备份
        self.create_backup(file_path)?;

        // 尝试精确匹配
        if let Some(pos) = self.find_exact_match(&content, old_string) {
            let new_content = format!(
                "{}{}{}",
                &content[..pos],
                new_string,
                &content[pos + old_string.len()..]
            );
            
            std::fs::write(file_path, new_content)
                .map_err(|e| format!("Failed to write file: {}", e))?;

            // 计算行号
            let line_number = content[..pos].lines().count() + 1;

            return Ok(EditResult {
                success: true,
                message: format!("Successfully edited at line {}", line_number),
                old_string: old_string.to_string(),
                new_string: new_string.to_string(),
                line_number: Some(line_number),
            });
        }

        // 尝试缩进容忍匹配
        if let Some((pos, actual_old)) = self.find_with_indentation_tolerance(&content, old_string) {
            let new_content = format!(
                "{}{}{}",
                &content[..pos],
                new_string,
                &content[pos + actual_old.len()..]
            );
            
            std::fs::write(file_path, new_content)
                .map_err(|e| format!("Failed to write file: {}", e))?;

            let line_number = content[..pos].lines().count() + 1;

            return Ok(EditResult {
                success: true,
                message: format!(
                    "Successfully edited at line {} (with indentation tolerance)",
                    line_number
                ),
                old_string: actual_old,
                new_string: new_string.to_string(),
                line_number: Some(line_number),
            });
        }

        // 未找到匹配
        Err(format!(
            "Could not find the specified text in file. \
             The old_string must match exactly (excluding leading whitespace differences)."
        ))
    }

    fn perform_multi_edit(
        &self,
        file_path: &Path,
        edits: Vec<(String, String)>,
    ) -> Result<Vec<EditResult>, String> {
        let mut results = Vec::new();
        
        for (old_string, new_string) in edits {
            match self.perform_edit(file_path, &old_string, &new_string) {
                Ok(result) => results.push(result),
                Err(e) => {
                    results.push(EditResult {
                        success: false,
                        message: e,
                        old_string,
                        new_string,
                        line_number: None,
                    });
                }
            }
        }
        
        Ok(results)
    }
}

#[async_trait]
impl Tool for CodeEditTool {
    fn name(&self) -> &str {
        "code_edit"
    }

    fn description(&self) -> &str {
        r#"编辑代码文件，将旧字符串替换为新字符串。

参数（单条编辑）:
- file_path: 文件路径
- old_string: 要替换的文本（必须完全匹配）
- new_string: 新文本

参数（批量编辑）:
- file_path: 文件路径
- edits: 编辑列表，每项为 { "old_string": "...", "new_string": "..." }

注意:
- old_string 必须完全匹配文件中的内容
- 支持缩进容忍（忽略前导空格差异）
- 会自动创建 .bak 备份文件

示例:
{"file_path": "src/main.rs", "old_string": "fn old() {}", "new_string": "fn new() {}"}"#
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: file_path")?;

        let validated_path = self.validate_path(file_path)?;

        if !validated_path.exists() {
            return Err(format!("File not found: {}", validated_path.display()));
        }

        // 检查是否有批量编辑
        if let Some(edits_array) = args.get("edits").and_then(|v| v.as_array()) {
            let mut edit_pairs = Vec::new();
            
            for edit in edits_array {
                let old = edit
                    .get("old_string")
                    .and_then(|v| v.as_str())
                    .ok_or("Each edit must have old_string")?;
                let new = edit
                    .get("new_string")
                    .and_then(|v| v.as_str())
                    .ok_or("Each edit must have new_string")?;
                edit_pairs.push((old.to_string(), new.to_string()));
            }

            let results = self.perform_multi_edit(&validated_path, edit_pairs)?;
            
            let success_count = results.iter().filter(|r| r.success).count();
            let fail_count = results.len() - success_count;

            let mut output = format!(
                "Multi-edit results: {} succeeded, {} failed\n",
                success_count, fail_count
            );
            output.push_str(&"-".repeat(60));
            output.push('\n');

            for (i, result) in results.iter().enumerate() {
                output.push_str(&format!("Edit {}: {}\n", i + 1, result.message));
            }

            if fail_count > 0 {
                return Err(output);
            }
            return Ok(output);
        }

        // 单条编辑
        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: old_string")?;

        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: new_string")?;

        let result = self.perform_edit(&validated_path, old_string, new_string)?;
        
        Ok(format!(
            "✓ {}\nFile: {}\nLine: {}",
            result.message,
            validated_path.display(),
            result.line_number.unwrap_or(0)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_test_file(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn test_exact_match() {
        let test_dir = std::path::PathBuf::from("./target/test_code_edit");
        std::fs::create_dir_all(&test_dir).unwrap();
        
        let file_path = create_test_file(&test_dir, "test1.rs", "fn main() {\n    println!(\"Hello\");\n}\n");

        let tool = CodeEditTool::new(&test_dir).with_backup(false);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let args = serde_json::json!({
                "file_path": "test1.rs",
                "old_string": "    println!(\"Hello\");",
                "new_string": "    println!(\"World\");"
            });

            let result = tool.execute(args).await;
            assert!(result.is_ok(), "{:?}", result);
        });

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("println!(\"World\")"));
        
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn test_indentation_tolerance() {
        let test_dir = std::path::PathBuf::from("./target/test_code_edit2");
        std::fs::create_dir_all(&test_dir).unwrap();
        
        let file_path = create_test_file(&test_dir, "test2.rs", "fn main() {\n    println!(\"Hello\");\n}\n");

        let tool = CodeEditTool::new(&test_dir).with_backup(false);

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let args = serde_json::json!({
                "file_path": "test2.rs",
                "old_string": "println!(\"Hello\");",
                "new_string": "println!(\"World\");"
            });

            let result = tool.execute(args).await;
            assert!(result.is_ok(), "{:?}", result);
        });

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("println!(\"World\")"));
        
        std::fs::remove_dir_all(&test_dir).ok();
    }
}
