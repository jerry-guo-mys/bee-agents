use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use crate::tools::Tool;

pub struct GitCommitTool {
    project_root: PathBuf,
}

impl GitCommitTool {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
        }
    }
}

#[async_trait]
impl Tool for GitCommitTool {
    fn name(&self) -> &str {
        "git_commit"
    }

    fn description(&self) -> &str {
        r#"执行 git add 和 commit 保存代码修改。

参数:
- message: 提交信息（必需）
- files: 要添加的文件列表（可选，默认 "."）

返回: 提交结果

示例:
{"message": "Fix bug in parser", "files": ["src/parser.rs"]}"#
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or("Missing required parameter: message")?;

        let files = args.get("files").and_then(|v| v.as_array());

        // Git add
        let mut add_cmd = Command::new("git");
        add_cmd.arg("add");
        add_cmd.current_dir(&self.project_root);

        if let Some(file_list) = files {
            for file in file_list {
                if let Some(f) = file.as_str() {
                    add_cmd.arg(f);
                }
            }
        } else {
            add_cmd.arg(".");
        }

        let add_output = add_cmd
            .output()
            .await
            .map_err(|e| format!("Failed to run git add: {}", e))?;

        if !add_output.status.success() {
            let stderr = String::from_utf8_lossy(&add_output.stderr);
            return Err(format!("git add failed: {}", stderr));
        }

        // Git commit
        let mut commit_cmd = Command::new("git");
        commit_cmd.arg("commit");
        commit_cmd.arg("-m");
        commit_cmd.arg(message);
        commit_cmd.current_dir(&self.project_root);

        let commit_output = commit_cmd
            .output()
            .await
            .map_err(|e| format!("Failed to run git commit: {}", e))?;

        let stdout = String::from_utf8_lossy(&commit_output.stdout);
        let stderr = String::from_utf8_lossy(&commit_output.stderr);

        if commit_output.status.success() {
            Ok(format!("✓ Committed: {}\n{}", message, stdout))
        } else {
            Err(format!("git commit failed: {}", stderr))
        }
    }
}
