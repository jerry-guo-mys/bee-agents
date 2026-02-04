//! 技能插件工具：由配置 [[tools.plugins]] 注册，运行「程序 + 参数模板」实现动态扩展
//!
//! 参数模板中 {{workspace}} 替换为沙箱根路径，{{key}} 从 LLM 传入的 args 中取 key；
//! 执行时无 shell，直接 exec program + substituted args，带超时与审计日志。

use std::path::Path;

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use crate::config::PluginEntry;
use crate::tools::Tool;

/// 从配置项构建的插件工具
pub struct PluginTool {
    name: String,
    description: String,
    program: String,
    args_template: Vec<String>,
    workspace: std::path::PathBuf,
    timeout_secs: u64,
}

impl PluginTool {
    /// 从配置条目与工作区路径、超时创建
    pub fn new(entry: &PluginEntry, workspace: &Path, timeout_secs: u64) -> Self {
        Self {
            name: entry.name.clone(),
            description: entry.description.clone(),
            program: entry.program.clone(),
            args_template: entry.args.clone(),
            workspace: workspace.to_path_buf(),
            timeout_secs,
        }
    }

    /// 替换模板中的 {{workspace}} 和 {{key}}；args 为 LLM 传入的 JSON 对象
    fn substitute(&self, args: &Value) -> Vec<String> {
        let workspace_str = self.workspace.to_string_lossy();
        let empty = serde_json::Map::new();
        let obj = args.as_object().unwrap_or(&empty);
        self.args_template
            .iter()
            .map(|tpl| {
                let mut s = tpl.clone();
                s = s.replace("{{workspace}}", &workspace_str);
                for (k, v) in obj {
                    let placeholder = format!("{{{{{}}}}}", k);
                    let val: String = match v {
                        Value::String(x) => x.clone(),
                        _ => v.to_string(),
                    };
                    s = s.replace(&placeholder, &val);
                }
                s
            })
            .collect()
    }
}

#[async_trait]
impl Tool for PluginTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let args_vec = self.substitute(&args);
        let program = self.program.clone();
        tracing::info!(tool = %self.name, program = %program, "plugin tool invoke");
        let child = Command::new(&program)
            .args(&args_vec)
            .current_dir(&self.workspace)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("plugin spawn failed: {}", e))?;
        let timeout = std::time::Duration::from_secs(self.timeout_secs);
        let output = tokio::time::timeout(timeout, child.wait_with_output())
            .await
            .map_err(|_| format!("plugin timeout after {}s", self.timeout_secs))?
            .map_err(|e| format!("plugin wait failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            return Err(format!(
                "exit {:?}: stderr {}",
                output.status.code(),
                stderr.trim()
            ));
        }
        Ok(stdout.trim().to_string())
    }
}
