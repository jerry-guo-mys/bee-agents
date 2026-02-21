//! 沙箱文件系统工具
//!
//! SafeFs 绑定 root_dir，所有路径经 resolve 校验必须在 root 下（禁止 ../ 逃逸）；
//! CatTool / LsTool 基于 SafeFs 提供 cat / ls 能力。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;

use crate::core::AgentError;
use crate::tools::Tool;

/// 沙箱文件系统：绑定根目录，resolve 校验路径在根下，防止路径逃逸
#[derive(Debug, Clone)]
pub struct SafeFs {
    root_dir: PathBuf,
}

impl SafeFs {
    pub fn new(root_dir: impl AsRef<Path>) -> Self {
        let root = root_dir.as_ref().to_path_buf();
        let root_dir = root.canonicalize().unwrap_or(root);
        Self { root_dir }
    }

    /// 检查路径是否在沙箱内
    pub fn resolve(&self, path: &str) -> Result<PathBuf, AgentError> {
        let path = path.trim_start_matches("./");
        let full = self.root_dir.join(path);
        let canonical = full
            .canonicalize()
            .map_err(|_| AgentError::ToolExecutionFailed(format!("Path not found: {}", path)))?;
        let root_canon = self
            .root_dir
            .canonicalize()
            .unwrap_or_else(|_| self.root_dir.clone());
        if canonical.starts_with(root_canon) {
            Ok(canonical)
        } else {
            Err(AgentError::PathEscape(path.to_string())) // 如 ../../etc/passwd
        }
    }

    pub fn read_file(&self, path: &str) -> Result<String, AgentError> {
        let resolved = self.resolve(path)?;
        std::fs::read_to_string(&resolved).map_err(|e| {
            AgentError::ToolExecutionFailed(format!("Read failed: {}", e))
        })
    }

    pub fn list_dir(&self, path: &str) -> Result<Vec<String>, AgentError> {
        let base = if path.is_empty() || path == "." {
            self.root_dir.clone()
        } else {
            self.resolve(path)?
        };
        let mut entries = Vec::new();
        for e in std::fs::read_dir(&base).map_err(|e| {
            AgentError::ToolExecutionFailed(format!("List failed: {}", e))
        })? {
            let e = e.map_err(|e| AgentError::ToolExecutionFailed(e.to_string()))?;
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with('.') {
                let ty = if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    "/"
                } else {
                    ""
                };
                entries.push(format!("{}{}", name, ty));
            }
        }
        entries.sort();
        Ok(entries)
    }
}

/// Cat 工具：读取文件内容
pub struct CatTool {
    fs: SafeFs,
}

impl CatTool {
    pub fn new(root_dir: impl AsRef<Path>) -> Self {
        Self {
            fs: SafeFs::new(root_dir),
        }
    }
}

#[async_trait]
impl Tool for CatTool {
    fn name(&self) -> &str {
        "cat"
    }

    fn description(&self) -> &str {
        "Read file contents. Args: {\"path\": \"file path relative to workspace\"}"
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        tracing::info!(path = %path, "cat tool execute");
        self.fs.read_file(path).map_err(|e| e.to_string())
    }
}

/// Ls 工具：列出目录
pub struct LsTool {
    fs: SafeFs,
}

impl LsTool {
    pub fn new(root_dir: impl AsRef<Path>) -> Self {
        Self {
            fs: SafeFs::new(root_dir),
        }
    }
}

#[async_trait]
impl Tool for LsTool {
    fn name(&self) -> &str {
        "ls"
    }

    fn description(&self) -> &str {
        "List directory. Args: {\"path\": \"directory path, default '.'\"}"
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        tracing::info!(path = %path, "ls tool execute");
        let entries = self.fs.list_dir(path).map_err(|e| e.to_string())?;
        Ok(entries.join("\n"))
    }
}
