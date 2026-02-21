use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use std::process::Command;

use crate::tools::Tool;

pub struct GitDiffTool;

impl Default for GitDiffTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GitDiffTool {
    pub fn new() -> Self {
        Self
    }

    fn run_git_command(&self, args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
        let mut cmd = Command::new("git");
        cmd.args(args);
        
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        
        let output = cmd.output().map_err(|e| format!("Failed to run git: {}", e))?;
        
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Git command failed: {}", stderr));
        }
        
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn is_git_repo(&self, path: &Path) -> bool {
        path.join(".git").exists()
    }
}

#[async_trait]
impl Tool for GitDiffTool {
    fn name(&self) -> &str {
        "git_diff"
    }

    fn description(&self) -> &str {
        "Show git diff. Args: {\"path\": \"repo path\", \"mode\": \"unstaged|staged|commit|branch\", \"target\": \"commit/branch\", \"base\": \"HEAD\", \"file\": \"specific file\", \"stat\": false}"
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let path_str = args["path"].as_str().unwrap_or(".");
        let path = Path::new(path_str);
        let mode = args["mode"].as_str().unwrap_or("unstaged");
        let stat = args["stat"].as_bool().unwrap_or(false);
        
        if !self.is_git_repo(path) {
            return Err(format!(
                "Not a git repository: {}. Run 'git init' first.",
                path.display()
            ));
        }
        
        let mut git_args = vec!["diff"];
        
        if stat {
            git_args.push("--stat");
        }
        
        match mode {
            "unstaged" => {}
            "staged" | "cached" => {
                git_args.push("--cached");
            }
            "commit" => {
                let target = args["target"].as_str().ok_or({
                    "'target' is required for commit mode"
                })?;
                let base = args["base"].as_str().unwrap_or("HEAD");
                git_args.push(base);
                git_args.push(target);
            }
            "branch" => {
                let target = args["target"].as_str().ok_or({
                    "'target' is required for branch mode"
                })?;
                let base = args["base"].as_str().unwrap_or("HEAD");
                git_args.push(base);
                git_args.push(target);
            }
            _ => return Err(format!("Invalid mode: {}", mode)),
        }
        
        if let Some(file) = args["file"].as_str() {
            git_args.push("--");
            git_args.push(file);
        }
        
        let diff_output = match self.run_git_command(&git_args, Some(path)) {
            Ok(output) => output,
            Err(e) => return Err(format!("Git command failed: {}", e)),
        };
        
        if diff_output.is_empty() {
            let message = match mode {
                "unstaged" => "No unstaged changes.".to_string(),
                "staged" | "cached" => "No staged changes.".to_string(),
                "commit" | "branch" => "No differences found.".to_string(),
                _ => "No changes found.".to_string(),
            };
            return Ok(message);
        }
        
        let header = format!("## Git Diff ({})\n\n", mode);
        Ok(format!("{}{}", header, diff_output))
    }
}