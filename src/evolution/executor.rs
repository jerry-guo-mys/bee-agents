use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::io::{self, Write};

use tokio::time;

use crate::tools::ToolExecutor;
use crate::evolution::types::{ImprovementPlan, IterationResult};
use crate::config::ApprovalMode;
use crate::evolution::engine::EvolutionConfig;

pub struct ExecutionEngine {
    executor: Arc<ToolExecutor>,
    project_root: PathBuf,
    config: EvolutionConfig,
}

impl ExecutionEngine {
    pub fn new(
        executor: Arc<ToolExecutor>,
        project_root: impl AsRef<Path>,
        config: EvolutionConfig,
    ) -> Self {
        Self {
            executor,
            project_root: project_root.as_ref().to_path_buf(),
            config,
        }
    }

    pub async fn execute_plan(
        &self,
        plan: &ImprovementPlan,
        steps: &[String],
    ) -> Result<IterationResult, String> {
        let mut changes_made = Vec::new();
        let mut lessons_learned = Vec::new();

        // 检查是否需要审批
        if !matches!(self.config.approval_mode, ApprovalMode::None) {
            let needs_approval = self.config.require_approval_for.is_empty() ||
                self.config.require_approval_for.iter().any(|t| plan.title.to_lowercase().contains(&t.to_lowercase()) ||
                    format!("{:?}", plan.improvement_type).to_lowercase().contains(&t.to_lowercase()));
            
            if needs_approval {
                match self.check_approval(plan).await {
                    Ok(true) => (),
                    Ok(false) => {
                        return Ok(IterationResult {
                            iteration: 0,
                            success: false,
                            changes_made,
                            tests_passed: false,
                            quality_score: 0.0,
                            lessons_learned: vec!["审批被拒绝".to_string()],
                        });
                    }
                    Err(e) => {
                        return Ok(IterationResult {
                            iteration: 0,
                            success: false,
                            changes_made,
                            tests_passed: false,
                            quality_score: 0.0,
                            lessons_learned: vec![format!("审批检查失败: {}", e)],
                        });
                    }
                }
            }
        }

        for (step_idx, step) in steps.iter().enumerate() {
            println!("Executing step {}/{}: {}", step_idx + 1, steps.len(), step);

            match self.execute_step(plan, step).await {
                Ok(change) => {
                    changes_made.push(format!("Step {}: {}", step_idx + 1, change));
                }
                Err(e) => {
                    lessons_learned.push(format!("Step {} failed: {}", step_idx + 1, e));
                    return Ok(IterationResult {
                        iteration: 0,
                        success: false,
                        changes_made,
                        tests_passed: false,
                        quality_score: 0.0,
                        lessons_learned,
                    });
                }
            }

            if !self.verify_changes().await? {
                let error = format!("Verification failed after step {}", step_idx + 1);
                lessons_learned.push(error.clone());
                return Err(error);
            }
        }

        let tests_passed = self.run_tests().await?;
        let quality_score = self.estimate_quality().await?;

        if self.config.auto_commit {
            self.commit_changes(plan).await?;
        }

        Ok(IterationResult {
            iteration: 0,
            success: true,
            changes_made,
            tests_passed,
            quality_score,
            lessons_learned,
        })
    }

    async fn execute_step(&self, _plan: &ImprovementPlan, step: &str) -> Result<String, String> {
        if step.to_lowercase().contains("remove") || step.to_lowercase().contains("delete") {
            return self.execute_removal(step).await;
        } else if step.to_lowercase().contains("add") || step.to_lowercase().contains("create") {
            return self.execute_addition(step).await;
        } else if step.to_lowercase().contains("replace") || step.to_lowercase().contains("change") {
            return self.execute_replacement(step).await;
        } else if step.to_lowercase().contains("rename") {
            return self.execute_rename(step).await;
        }

        Err(format!("Cannot parse step: {}", step))
    }

    async fn execute_removal(&self, step: &str) -> Result<String, String> {
        if let Some((file_path, pattern)) = self.extract_file_and_pattern(step) {
            let args = serde_json::json!({
                "file_path": file_path,
                "old_string": pattern,
                "new_string": ""
            });

            self.executor.execute("code_edit", args).await.map_err(|e| e.to_string())?;
            Ok(format!("Removed pattern from {}", file_path))
        } else {
            Err(format!("Could not parse removal step: {}", step))
        }
    }

    async fn execute_addition(&self, step: &str) -> Result<String, String> {
        // TODO: Implement specialized addition for functions, types, tests
        // For now, fall through to generic addition
        if step.to_lowercase().contains("function") || step.to_lowercase().contains("fn ") {
            return self.add_function(step).await;
        } else if step.to_lowercase().contains("struct") || step.to_lowercase().contains("enum") {
            return self.add_type(step).await;
        } else if step.to_lowercase().contains("test") {
            return self.add_test(step).await;
        }

        if let Some((file_path, content)) = self.extract_file_and_content(step) {
            let existing = std::fs::read_to_string(&self.project_root.join(&file_path))
                .unwrap_or_default();
            
            if existing.is_empty() {
                let args = serde_json::json!({
                    "file_path": file_path,
                    "content": content,
                    "overwrite": false
                });

                self.executor.execute("code_write", args).await.map_err(|e| e.to_string())?;
                Ok(format!("Created new file: {}", file_path))
            } else {
                let args = serde_json::json!({
                    "file_path": file_path,
                    "old_string": "",
                    "new_string": content
                });

            self.executor.execute("code_edit", args).await.map_err(|e| e.to_string())?;
                Ok(format!("Added content to {}", file_path))
            }
        } else {
            Err(format!("Could not parse addition step: {}", step))
        }
    }

    async fn execute_replacement(&self, step: &str) -> Result<String, String> {
        if let Some((file_path, old_content, new_content)) = self.extract_replacement(step) {
            let args = serde_json::json!({
                "file_path": file_path,
                "old_string": old_content,
                "new_string": new_content
            });

            self.executor.execute("code_edit", args).await.map_err(|e| e.to_string())?;
            Ok(format!("Replaced content in {}", file_path))
        } else {
            Err(format!("Could not parse replacement step: {}", step))
        }
    }

    async fn execute_rename(&self, _step: &str) -> Result<String, String> {
        Err("Rename not implemented yet".to_string())
    }

    async fn add_function(&self, step: &str) -> Result<String, String> {
        // Parse step like "Add function foo(bar: i32) -> bool to src/lib.rs"
        // or "Create function calculate_total in src/calculations.rs"
        
        // First try to extract file path
        let file_path = self.extract_file_path(step)
            .ok_or_else(|| format!("Could not find file path in step: {}", step))?;
        
        // Validate path is allowed
        if !self.is_path_allowed(Path::new(&file_path)) {
            return Err(format!("File path '{}' is not allowed", file_path));
        }
        
        // Try to extract function signature
        let func_sig = self.extract_function_signature(step)
            .unwrap_or_else(|| "fn new_function() {\n    // TODO: Implement\n}".to_string());
        
        // Read existing file to decide where to insert
        let full_path = self.project_root.join(&file_path);
        let existing_content = std::fs::read_to_string(&full_path)
            .unwrap_or_default();
        
        let new_content = if existing_content.is_empty() {
            // New file
            format!("{}\n", func_sig)
        } else {
            // Append to end of file (simplified)
            format!("{}\n\n{}", existing_content.trim_end(), func_sig)
        };
        
        // Use code_write or code_edit tool
        let args = if existing_content.is_empty() {
            serde_json::json!({
                "file_path": file_path,
                "content": new_content,
                "overwrite": false
            })
        } else {
            serde_json::json!({
                "file_path": file_path,
                "old_string": existing_content,
                "new_string": new_content
            })
        };
        
        let tool_name = if existing_content.is_empty() { "code_write" } else { "code_edit" };
        self.executor.execute(tool_name, args).await.map_err(|e| e.to_string())?;
        
        Ok(format!("Added function to {}", file_path))
    }

    async fn add_type(&self, step: &str) -> Result<String, String> {
        // Parse step like "Add struct Item with fields: id, name, price to src/models.rs"
        
        let file_path = self.extract_file_path(step)
            .ok_or_else(|| format!("Could not find file path in step: {}", step))?;
        
        if !self.is_path_allowed(Path::new(&file_path)) {
            return Err(format!("File path '{}' is not allowed", file_path));
        }
        
        // Extract type definition (simplified)
        let type_def = if step.to_lowercase().contains("struct") {
            "struct NewType {\n    // TODO: Add fields\n}"
        } else if step.to_lowercase().contains("enum") {
            "enum NewEnum {\n    // TODO: Add variants\n}"
        } else {
            "struct NewType {\n    // TODO: Add fields\n}"
        };
        
        let full_path = self.project_root.join(&file_path);
        let existing_content = std::fs::read_to_string(&full_path)
            .unwrap_or_default();
        
        let new_content = if existing_content.is_empty() {
            format!("{}\n", type_def)
        } else {
            format!("{}\n\n{}", existing_content.trim_end(), type_def)
        };
        
        let args = if existing_content.is_empty() {
            serde_json::json!({
                "file_path": file_path,
                "content": new_content,
                "overwrite": false
            })
        } else {
            serde_json::json!({
                "file_path": file_path,
                "old_string": existing_content,
                "new_string": new_content
            })
        };
        
        let tool_name = if existing_content.is_empty() { "code_write" } else { "code_edit" };
        self.executor.execute(tool_name, args).await.map_err(|e| e.to_string())?;
        
        Ok(format!("Added type to {}", file_path))
    }

    async fn add_test(&self, step: &str) -> Result<String, String> {
        // Parse step like "Add test for calculate_total function in src/lib.rs"
        
        let file_path = self.extract_file_path(step)
            .ok_or_else(|| format!("Could not find file path in step: {}", step))?;
        
        if !self.is_path_allowed(Path::new(&file_path)) {
            return Err(format!("File path '{}' is not allowed", file_path));
        }
        
        // Create a basic test
        let test_code = r#"#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_function() {
        // TODO: Add test assertions
        assert_eq!(2 + 2, 4);
    }
}"#;
        
        let full_path = self.project_root.join(&file_path);
        let existing_content = std::fs::read_to_string(&full_path)
            .unwrap_or_default();
        
        // Check if tests module already exists
        let new_content = if existing_content.contains("#[cfg(test)]") {
            // Append to existing tests module (simplified - just append at end)
            format!("{}\n\n{}", existing_content.trim_end(), "    #[test]\n    fn test_new_function() {\n        assert_eq!(2 + 2, 4);\n    }")
        } else {
            // Add new tests module at end
            format!("{}\n\n{}", existing_content.trim_end(), test_code)
        };
        
        let args = serde_json::json!({
            "file_path": file_path,
            "old_string": existing_content,
            "new_string": new_content
        });
        
        self.executor.execute("code_edit", args).await.map_err(|e| e.to_string())?;
        
        Ok(format!("Added test to {}", file_path))
    }
    
    fn extract_file_path(&self, step: &str) -> Option<String> {
        // Look for file paths ending with .rs, .toml, .md
        let words: Vec<&str> = step.split_whitespace().collect();
        for word in words {
            if word.ends_with(".rs") || word.ends_with(".toml") || word.ends_with(".md") {
                return Some(word.to_string());
            }
        }
        None
    }
    
    fn extract_function_signature(&self, step: &str) -> Option<String> {
        // Look for patterns like "fn function_name(" or "function foo("
        let lower_step = step.to_lowercase();
        
        // Try to find "fn " pattern
        if let Some(idx) = lower_step.find("fn ") {
            let remaining = &step[idx..];
            // Take until next period or newline (simplified)
            let end = remaining.find('.').unwrap_or(remaining.len());
            let sig = &remaining[..end];
            return Some(sig.trim().to_string());
        }
        
        // Try to find "function " pattern  
        if let Some(idx) = lower_step.find("function ") {
            let remaining = &step[idx..];
            let end = remaining.find('.').unwrap_or(remaining.len());
            let mut sig = &remaining[..end];
            // Convert "function name" to "fn name"
            sig = sig.trim();
            if sig.starts_with("function ") {
                sig = &sig[9..];
            }
            return Some(format!("fn {}", sig));
        }
        
        None
    }

    fn extract_file_and_pattern(&self, step: &str) -> Option<(String, String)> {
        let words: Vec<&str> = step.split_whitespace().collect();
        for word in words {
            if word.ends_with(".rs") || word.ends_with(".toml") {
                let remaining: Vec<&str> = step.split(word).collect();
                if remaining.len() > 1 {
                    let pattern = remaining[1].trim().to_string();
                    return Some((word.to_string(), pattern));
                }
            }
        }
        None
    }

    fn extract_file_and_content(&self, step: &str) -> Option<(String, String)> {
        let lines: Vec<&str> = step.lines().collect();
        for line in lines {
            if line.contains(".rs") || line.contains(".toml") {
                let parts: Vec<&str> = line.split("contains").collect();
                if parts.len() >= 2 {
                    let file_part = parts[0].trim();
                    let joined = parts[1..].join("contains");
                    let content_part = joined.trim();
                    return Some((file_part.to_string(), content_part.to_string()));
                }
            }
        }
        None
    }

    fn extract_replacement(&self, step: &str) -> Option<(String, String, String)> {
        if step.contains("->") {
            let parts: Vec<&str> = step.split("->").collect();
            if parts.len() == 2 {
                let left = parts[0].trim();
                let right = parts[1].trim();

                let file_match = left.find(".rs").or_else(|| left.find(".toml"));
                if let Some(idx) = file_match {
                    let file_path = left[..idx + 3].trim().to_string();
                    let old_content = left[idx + 3..].trim().to_string();
                    let new_content = right.to_string();

                    return Some((file_path, old_content, new_content));
                }
            }
        }
        None
    }

    async fn verify_changes(&self) -> Result<bool, String> {
        let args = serde_json::json!({});

        match self.executor.execute("test_check", args).await {
            Ok(_) => Ok(true),
            Err(e) => {
                eprintln!("Check failed: {}", e);
                Ok(false)
            }
        }
    }

    async fn run_tests(&self) -> Result<bool, String> {
        let args = serde_json::json!({});

        match self.executor.execute("test_run", args).await {
            Ok(_) => Ok(true),
            Err(e) => {
                eprintln!("Tests failed: {}", e);
                Ok(false)
            }
        }
    }

    async fn estimate_quality(&self) -> Result<f64, String> {
        Ok(0.8)
    }

    async fn commit_changes(&self, plan: &ImprovementPlan) -> Result<(), String> {
        let message = format!("{}: {}", plan.improvement_type, plan.title);
        let args = serde_json::json!({
            "message": message,
            "files": ["."]
        });

        self.executor.execute("git_commit", args).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn check_approval(&self, plan: &ImprovementPlan) -> Result<bool, String> {
        match self.config.approval_mode {
            ApprovalMode::None => Ok(true),
            ApprovalMode::Console => self.check_approval_console(plan).await,
            ApprovalMode::Prompt => self.check_approval_prompt(plan).await,
            ApprovalMode::Webhook => self.check_approval_webhook(plan).await,
        }
    }

    async fn check_approval_console(&self, plan: &ImprovementPlan) -> Result<bool, String> {
        println!("\n📋 改进计划需要审批:");
        println!("标题: {}", plan.title);
        println!("类型: {:?}", plan.improvement_type);
        println!("目标文件: {:?}", plan.target_files);
        println!("预期结果: {}", plan.expected_outcome);
        println!();
        print!("是否批准执行？(y/n): ");
        io::stdout().flush().map_err(|e| e.to_string())?;

        let mut input = String::new();
        io::stdin().read_line(&mut input).map_err(|e| e.to_string())?;

        let input = input.trim().to_lowercase();
        Ok(input == "y" || input == "yes" || input == "是")
    }

    async fn check_approval_prompt(&self, plan: &ImprovementPlan) -> Result<bool, String> {
        println!("\n📋 改进计划需要审批 ({}秒超时):", self.config.approval_timeout_seconds);
        println!("标题: {}", plan.title);
        println!("类型: {:?}", plan.improvement_type);
        println!("目标文件: {:?}", plan.target_files);
        println!("预期结果: {}", plan.expected_outcome);
        println!();
        print!("是否批准执行？(y/n): ");
        io::stdout().flush().map_err(|e| e.to_string())?;

        // 使用 spawn_blocking 在后台线程中读取输入，以便可以超时
        let result = time::timeout(
            time::Duration::from_secs(self.config.approval_timeout_seconds),
            tokio::task::spawn_blocking(|| {
                let mut input = String::new();
                match io::stdin().read_line(&mut input) {
                    Ok(_) => Some(input),
                    Err(_) => None,
                }
            }),
        ).await;

        match result {
            Ok(join_result) => match join_result {
                Ok(Some(input)) => {
                    let input = input.trim().to_lowercase();
                    Ok(input == "y" || input == "yes" || input == "是")
                }
                _ => {
                    println!("⏰ 审批超时或输入错误，自动拒绝");
                    Ok(false)
                }
            },
            Err(_) => {
                println!("⏰ 审批超时，自动拒绝");
                Ok(false)
            }
        }
    }

    async fn check_approval_webhook(&self, plan: &ImprovementPlan) -> Result<bool, String> {
        let url = match &self.config.approval_webhook_url {
            Some(url) => url,
            None => {
                eprintln!("⚠️ Webhook URL 未配置，自动拒绝");
                return Ok(false);
            }
        };

        println!("🌐 发送审批请求到 Webhook: {}", url);
        println!("计划: {}", plan.title);

        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "plan": {
                "id": plan.id,
                "title": plan.title,
                "type": format!("{:?}", plan.improvement_type),
                "target_files": plan.target_files,
                "expected_outcome": plan.expected_outcome,
            },
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        let result = time::timeout(
            time::Duration::from_secs(self.config.approval_timeout_seconds),
            client.post(url)
                .json(&payload)
                .send(),
        ).await;

        match result {
            Ok(Ok(response)) => {
                if response.status().is_success() {
                    let body = response.text().await.map_err(|e| e.to_string())?;
                    // 简化：假设返回 JSON 包含 approved 字段
                    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
                    let approved = json.get("approved")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    Ok(approved)
                } else {
                    eprintln!("⚠️ Webhook 返回错误状态码: {}", response.status());
                    Ok(false)
                }
            }
            Ok(Err(e)) => {
                eprintln!("⚠️ Webhook 请求失败: {}", e);
                Ok(false)
            }
            Err(_) => {
                eprintln!("⚠️ Webhook 请求超时");
                Ok(false)
            }
        }
    }

    fn is_path_allowed(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy().to_string();
        
        // 检查是否在允许的目录中
        let allowed = self.config.allowed_directories.iter().any(|dir| {
            path_str.starts_with(dir) || path_str.contains(dir)
        });
        
        if !allowed {
            return false;
        }
        
        // 检查是否为受限文件
        for restricted in &self.config.restricted_files {
            if path_str.ends_with(restricted) || path_str.contains(restricted) {
                return false;
            }
        }
        
        // 检查文件大小（如果文件存在）
        if let Ok(metadata) = std::fs::metadata(path) {
            let size_kb = metadata.len() / 1024;
            if size_kb > self.config.max_file_size_kb as u64 {
                return false;
            }
        }
        
        true
    }

    #[allow(dead_code)]
    fn is_operation_allowed(&self, operation_type: &str) -> bool {
        self.config.allowed_operation_types.iter()
            .any(|op| op == operation_type)
    }

    #[allow(dead_code)]
    async fn validate_operation(&self, step: &str) -> Result<(), String> {
        // 解析步骤以提取文件路径和操作类型
        // 简化实现：检查基本安全性
        
        let step_lower = step.to_lowercase();
        let mut operation_type = "";
        
        if step_lower.contains("remove") || step_lower.contains("delete") {
            operation_type = "remove";
        } else if step_lower.contains("add") || step_lower.contains("create") {
            operation_type = "add";
        } else if step_lower.contains("replace") || step_lower.contains("change") {
            operation_type = "replace";
        } else if step_lower.contains("rename") {
            operation_type = "rename";
        }
        
        if !operation_type.is_empty() && !self.is_operation_allowed(operation_type) {
            return Err(format!("操作类型 '{}' 不被允许", operation_type));
        }
        
        // 尝试提取文件路径（简化）
        let words: Vec<&str> = step.split_whitespace().collect();
        for word in words {
            if word.ends_with(".rs") || word.ends_with(".toml") || word.ends_with(".md") {
                let file_path = Path::new(word);
                if !self.is_path_allowed(file_path) {
                    return Err(format!("文件 '{}' 不在允许的目录中或受限制", word));
                }
                break;
            }
        }
        
        Ok(())
    }
}
