//! 代码写入工具 - 创建新代码文件
//!
//! 用于自主迭代时创建新文件

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::tools::Tool;

/// 代码写入工具
pub struct CodeWriteTool {
    allowed_root: PathBuf,
    max_file_size: usize,
}

impl CodeWriteTool {
    pub fn new(allowed_root: impl AsRef<Path>) -> Self {
        Self {
            allowed_root: allowed_root.as_ref().to_path_buf(),
            max_file_size: 10 * 1024 * 1024, // 10MB
        }
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

    fn ensure_parent_dir(&self, file_path: &Path) -> Result<(), String> {
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create parent directory: {}", e))?;
        }
        Ok(())
    }
}

#[async_trait]
impl Tool for CodeWriteTool {
    fn name(&self) -> &str {
        "code_write"
    }

    fn description(&self) -> &str {
        r#"写入或覆盖代码文件内容。

参数:
- file_path: 文件路径（相对或绝对）
- content: 文件内容
- overwrite: 是否覆盖已存在的文件（可选，默认 false）

注意:
- 会自动创建父目录
- 默认不会覆盖已存在的文件（除非 overwrite=true）

示例:
{"file_path": "src/new_module.rs", "content": "pub fn hello() {}", "overwrite": false}"#
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let file_path = args
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: file_path")?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: content")?;

        let overwrite = args
            .get("overwrite")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if content.len() > self.max_file_size {
            return Err(format!(
                "Content too large: {} bytes (max: {})",
                content.len(),
                self.max_file_size
            ));
        }

        let validated_path = self.validate_path(file_path)?;

        // 检查文件是否已存在
        if validated_path.exists() && !overwrite {
            return Err(format!(
                "File already exists: {}. Use overwrite=true to overwrite.",
                validated_path.display()
            ));
        }

        // 确保父目录存在
        self.ensure_parent_dir(&validated_path)?;

        // 写入文件
        std::fs::write(&validated_path, content)
            .map_err(|e| format!("Failed to write file: {}", e))?;

        let action = if validated_path.exists() && overwrite {
            "Overwritten"
        } else {
            "Created"
        };

        Ok(format!(
            "✓ {} file: {} ({} bytes)",
            action,
            validated_path.display(),
            content.len()
        ))
    }
}
