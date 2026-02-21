//! Shell 执行器：白名单命令，禁止危险操作
//!
//! 仅允许配置中的命令名（首词，如 ls、grep、cargo）；禁止 rm -rf、wget、chmod 777 等子串；
//! 执行通过 sh -c / cmd /C，带超时与 tracing 审计。

use std::collections::HashSet;

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use crate::tools::Tool;

/// 禁止的命令/子串（即使白名单中有同名，也不允许带这些参数）
const FORBIDDEN_SUBSTR: &[&str] = &[
    "rm -rf",
    "rm -fr",
    "rm -r",
    "wget ",
    "curl | sh",
    "chmod 777",
    "chmod +s",
    "mkfs",
    "dd if=",
    "> /dev/sd",
    ":(){ :|:& };:", // fork bomb
];

/// Shell 工具：仅允许白名单内命令
pub struct ShellTool {
    allowed_commands: HashSet<String>,
    timeout_secs: u64,
}

impl ShellTool {
    pub fn new(allowed_commands: Vec<String>, timeout_secs: u64) -> Self {
        let allowed_commands = allowed_commands
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect();
        Self {
            allowed_commands,
            timeout_secs,
        }
    }

    /// 解析命令：只取第一个 token 作为命令名
    fn command_name<'a>(&self, raw: &'a str) -> &'a str {
        raw.split_whitespace().next().unwrap_or("")
    }

    fn is_allowed(&self, raw: &str) -> Result<(), String> {
        let raw_lower = raw.to_lowercase();
        for forbidden in FORBIDDEN_SUBSTR {
            if raw_lower.contains(forbidden) {
                return Err(format!("Forbidden pattern: {}", forbidden));
            }
        }
        let name = self.command_name(&raw_lower);
        if name.is_empty() {
            return Err("Empty command".to_string());
        }
        if self.allowed_commands.contains(name) {
            return Ok(());
        }
        Err(format!("Command '{}' not in allowlist", name))
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Run a whitelisted shell command. Allowed commands: ls, grep, cat, head, tail, wc, find, cargo, rustc (configurable)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute (must be in allowlist)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        self.is_allowed(command)?;

        tracing::info!(command = %command, "shell tool execute");

        let mut cmd = if cfg!(target_os = "windows") {
            let mut c = Command::new("cmd");
            c.args(["/C", command]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", command]);
            c
        };

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            cmd.output(),
        )
        .await
        .map_err(|_| format!("Command timed out after {}s", self.timeout_secs))?
        .map_err(|e| format!("Execution failed: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(format!("Exit {:?}\nstderr: {}", output.status, stderr.trim()));
        }
        Ok(if stderr.is_empty() {
            stdout
        } else {
            format!("{}\nstderr: {}", stdout.trim(), stderr.trim())
        })
    }
}
