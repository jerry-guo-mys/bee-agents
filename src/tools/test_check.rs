use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use crate::tools::Tool;

pub struct TestCheckTool {
    project_root: PathBuf,
    timeout_secs: u64,
}

impl TestCheckTool {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            timeout_secs: 120,
        }
    }

    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }
}

#[async_trait]
impl Tool for TestCheckTool {
    fn name(&self) -> &str {
        "test_check"
    }

    fn description(&self) -> &str {
        r#"检查代码编译（cargo check）。

参数:
- features: 启用的特性（可选）
- all_targets: 是否检查所有目标（可选，默认 true）

返回: 编译检查结果

示例:
{"features": "web", "all_targets": true}"#
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let features = args.get("features").and_then(|v| v.as_str());
        let all_targets = args.get("all_targets").and_then(|v| v.as_bool()).unwrap_or(true);

        let mut cmd = Command::new("cargo");
        cmd.arg("check");
        cmd.current_dir(&self.project_root);

        if all_targets {
            cmd.arg("--all-targets");
        }

        if let Some(feat) = features {
            if !feat.is_empty() {
                cmd.arg("--features").arg(feat);
            }
        }

        let output = tokio::time::timeout(
            tokio::time::Duration::from_secs(self.timeout_secs),
            cmd.output(),
        )
        .await
        .map_err(|_| "Check timed out")?
        .map_err(|e| format!("Failed to run check: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let success = output.status.success();

        let mut result = format!(
            "Check Result: {}\n\n",
            if success { "✓ PASSED" } else { "✗ FAILED" }
        );

        if !stdout.is_empty() {
            result.push_str(&stdout);
        }

        if !stderr.is_empty() {
            result.push_str(&stderr);
        }

        if success {
            Ok(result)
        } else {
            Err(result)
        }
    }
}
