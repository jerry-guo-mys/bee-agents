use std::collections::HashMap;
use std::path::Path;

use async_trait::async_trait;
use regex::Regex;
use serde_json::Value;
use walkdir::WalkDir;

use crate::tools::Tool;

pub struct CodeReviewTool {
    allowed_extensions: Vec<String>,
    max_file_size: usize,
    max_files_per_review: usize,
}

impl CodeReviewTool {
    pub fn new(_workspace_root: impl AsRef<Path>) -> Self {
        Self {
            allowed_extensions: vec![
                "rs".to_string(),
                "py".to_string(),
                "js".to_string(),
                "ts".to_string(),
                "go".to_string(),
                "java".to_string(),
                "cpp".to_string(),
                "c".to_string(),
                "h".to_string(),
                "hpp".to_string(),
                "md".to_string(),
                "toml".to_string(),
                "yaml".to_string(),
                "yml".to_string(),
                "json".to_string(),
            ],
            max_file_size: 1024 * 1024,
            max_files_per_review: 20,
        }
    }

    fn validate_path(&self, path: &str) -> anyhow::Result<std::path::PathBuf> {
        let path = Path::new(path);
        if path.is_absolute() {
            return Err(anyhow::anyhow!("Absolute paths not allowed"));
        }
        Ok(path.to_path_buf())
    }

    fn analyze_code(&self, content: &str, file_ext: &str) -> Vec<Issue> {
        let mut issues = Vec::new();
        
        match file_ext {
            "rs" => self.analyze_rust(content, &mut issues),
            "py" => self.analyze_python(content, &mut issues),
            "js" | "ts" => self.analyze_javascript(content, &mut issues),
            _ => {}
        }
        
        self.analyze_common(content, &mut issues);
        issues
    }

    fn analyze_rust(&self, content: &str, issues: &mut Vec<Issue>) {
        let lines: Vec<&str> = content.lines().collect();
        let mut in_test_module = false;
        let mut bracket_depth = 0;
        
        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1;
            let trimmed = line.trim();
            
            bracket_depth += line.matches('{').count() as i32;
            bracket_depth -= line.matches('}').count() as i32;
            
            if trimmed.starts_with("#[cfg(test)]") {
                in_test_module = true;
            }
            if in_test_module && bracket_depth <= 0 {
                in_test_module = false;
            }
            
            if !in_test_module {
                if line.contains("unwrap()") {
                    issues.push(Issue {
                        line: line_num,
                        severity: "warning".to_string(),
                        category: "error_handling".to_string(),
                        message: "Consider using ? operator or proper error handling instead of unwrap()".to_string(),
                    });
                }
                
                if line.contains("expect(") && !line.contains("//") {
                    issues.push(Issue {
                        line: line_num,
                        severity: "info".to_string(),
                        category: "error_handling".to_string(),
                        message: "expect() with a descriptive message is okay, but consider if error recovery is possible".to_string(),
                    });
                }
            }
            
            if line.contains("todo!") || line.contains("unimplemented!") {
                issues.push(Issue {
                    line: line_num,
                    severity: "warning".to_string(),
                    category: "incomplete".to_string(),
                    message: "Found incomplete code (todo! or unimplemented!)".to_string(),
                });
            }
            
            if line.contains("println!") || line.contains("eprintln!") {
                issues.push(Issue {
                    line: line_num,
                    severity: "info".to_string(),
                    category: "logging".to_string(),
                    message: "Consider using tracing instead of println!/eprintln! for production code".to_string(),
                });
            }
            
            if line.contains("unsafe ") {
                issues.push(Issue {
                    line: line_num,
                    severity: "warning".to_string(),
                    category: "safety".to_string(),
                    message: "Unsafe block found - ensure safety invariants are documented".to_string(),
                });
            }
            
            if line.contains("panic!") && !line.contains("//") {
                issues.push(Issue {
                    line: line_num,
                    severity: "warning".to_string(),
                    category: "error_handling".to_string(),
                    message: "Avoid panic! in library code - return errors instead".to_string(),
                });
            }
            
            if line.contains("std::mem::forget") {
                issues.push(Issue {
                    line: line_num,
                    severity: "error".to_string(),
                    category: "safety".to_string(),
                    message: "std::mem::forget can cause memory leaks - use ManuallyDrop if necessary".to_string(),
                });
            }
            
            if trimmed.starts_with("fn ") && !trimmed.contains("-> ") && !trimmed.contains("{") {
                let next_line = lines.get(i + 1).map(|l| l.trim());
                if next_line.is_none_or(|l| !l.starts_with("->")) {
                    issues.push(Issue {
                        line: line_num,
                        severity: "info".to_string(),
                        category: "style".to_string(),
                        message: "Consider adding explicit return type to function".to_string(),
                    });
                }
            }
            
            if line.contains("clone()") && !line.contains("//") {
                let clone_count = line.matches("clone()").count();
                if clone_count > 2 {
                    issues.push(Issue {
                        line: line_num,
                        severity: "info".to_string(),
                        category: "performance".to_string(),
                        message: format!("Multiple clones detected ({}). Consider using references or Rc/Arc", clone_count),
                    });
                }
            }
            
            if trimmed.starts_with("if ") && line.contains(" == true") {
                issues.push(Issue {
                    line: line_num,
                    severity: "info".to_string(),
                    category: "style".to_string(),
                    message: "Redundant comparison: == true is unnecessary".to_string(),
                });
            }
            
            if trimmed.starts_with("if ") && line.contains(" == false") {
                issues.push(Issue {
                    line: line_num,
                    severity: "info".to_string(),
                    category: "style".to_string(),
                    message: "Consider using !condition instead of == false".to_string(),
                });
            }
            
            if line.contains("as ") && (line.contains("as i32") || line.contains("as u32") || line.contains("as i64") || line.contains("as u64"))
                && !line.contains("//") {
                    issues.push(Issue {
                        line: line_num,
                        severity: "info".to_string(),
                        category: "safety".to_string(),
                        message: "Consider using try_into() instead of as for safe numeric conversion".to_string(),
                    });
                }
        }
        
        let content_str = content.to_string();
        if content_str.contains("pub fn ") && !content_str.contains("///") && !content_str.contains("//!") {
            let pub_fn_re = Regex::new(r"pub fn ([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
            for cap in pub_fn_re.captures_iter(&content_str) {
                let fn_name = cap.get(1).unwrap().as_str();
                let fn_pos = cap.get(0).unwrap().start();
                let line_num = content_str[..fn_pos].lines().count() + 1;
                
                let has_docs_before = content_str[..fn_pos]
                    .lines()
                    .rev()
                    .take(5)
                    .any(|l| l.trim().starts_with("///") || l.trim().starts_with("//!"));
                
                if !has_docs_before {
                    issues.push(Issue {
                        line: line_num,
                        severity: "info".to_string(),
                        category: "documentation".to_string(),
                        message: format!("Public function '{}' lacks documentation comments", fn_name),
                    });
                }
            }
        }
        
        if content_str.contains("pub struct ") && !content_str.contains("#![") {
            let pub_struct_re = Regex::new(r"pub struct ([a-zA-Z_][a-zA-Z0-9_]*)").unwrap();
            for cap in pub_struct_re.captures_iter(&content_str) {
                let struct_name = cap.get(1).unwrap().as_str();
                let struct_pos = cap.get(0).unwrap().start();
                let line_num = content_str[..struct_pos].lines().count() + 1;
                
                let has_docs_before = content_str[..struct_pos]
                    .lines()
                    .rev()
                    .take(3)
                    .any(|l| l.trim().starts_with("///"));
                
                if !has_docs_before {
                    issues.push(Issue {
                        line: line_num,
                        severity: "info".to_string(),
                        category: "documentation".to_string(),
                        message: format!("Public struct '{}' lacks documentation", struct_name),
                    });
                }
            }
        }
        
        let mut struct_depth = 0;
        let mut current_struct = String::new();
        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1;
            let trimmed = line.trim();
            
            if trimmed.starts_with("pub struct ") {
                struct_depth = 1;
                current_struct = trimmed.split_whitespace().nth(2).unwrap_or("").to_string();
            } else if struct_depth > 0 {
                if line.contains('{') {
                    struct_depth += 1;
                }
                if line.contains('}') {
                    struct_depth -= 1;
                }
                
                if trimmed.starts_with("pub ") && trimmed.contains(":") && !trimmed.starts_with("///") {
                    let field_name = trimmed.split(':').next().unwrap_or("").trim();
                    if !field_name.is_empty() && !field_name.starts_with("///") {
                        let has_docs = if i > 0 {
                            lines[i - 1].trim().starts_with("///")
                        } else { false };
                        
                        if !has_docs {
                            issues.push(Issue {
                                line: line_num,
                                severity: "info".to_string(),
                                category: "documentation".to_string(),
                                message: format!("Public field '{}' in {} lacks documentation", field_name, current_struct),
                            });
                        }
                    }
                }
            }
        }
    }

    fn analyze_python(&self, content: &str, issues: &mut Vec<Issue>) {
        let lines: Vec<&str> = content.lines().collect();
        
        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1;
            
            if line.contains("print(") && !line.contains("#") {
                issues.push(Issue {
                    line: line_num,
                    severity: "info".to_string(),
                    category: "logging".to_string(),
                    message: "Consider using logging module instead of print()".to_string(),
                });
            }
            
            if line.contains("except:") && !line.contains("except Exception") {
                issues.push(Issue {
                    line: line_num,
                    severity: "warning".to_string(),
                    category: "error_handling".to_string(),
                    message: "Bare except: clause catches KeyboardInterrupt and SystemExit".to_string(),
                });
            }
            
            if line.contains("TODO") || line.contains("FIXME") {
                issues.push(Issue {
                    line: line_num,
                    severity: "info".to_string(),
                    category: "incomplete".to_string(),
                    message: "Found TODO/FIXME comment".to_string(),
                });
            }
        }
    }

    fn analyze_javascript(&self, content: &str, issues: &mut Vec<Issue>) {
        let lines: Vec<&str> = content.lines().collect();
        
        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1;
            
            if line.contains("console.log") && !line.contains("//") {
                issues.push(Issue {
                    line: line_num,
                    severity: "info".to_string(),
                    category: "logging".to_string(),
                    message: "Remove console.log before production".to_string(),
                });
            }
            
            if line.contains("debugger;") {
                issues.push(Issue {
                    line: line_num,
                    severity: "warning".to_string(),
                    category: "debugging".to_string(),
                    message: "Remove debugger statement before production".to_string(),
                });
            }
            
            if line.contains("eval(") {
                issues.push(Issue {
                    line: line_num,
                    severity: "error".to_string(),
                    category: "security".to_string(),
                    message: "Avoid using eval() - major security risk".to_string(),
                });
            }
            
            if line.contains("TODO") || line.contains("FIXME") || line.contains("XXX") {
                issues.push(Issue {
                    line: line_num,
                    severity: "info".to_string(),
                    category: "incomplete".to_string(),
                    message: "Found TODO/FIXME/XXX comment".to_string(),
                });
            }
        }
    }

    fn analyze_common(&self, content: &str, issues: &mut Vec<Issue>) {
        let lines: Vec<&str> = content.lines().collect();
        
        if lines.len() > 500 {
            issues.push(Issue {
                line: 1,
                severity: "info".to_string(),
                category: "complexity".to_string(),
                message: format!("File is {} lines - consider refactoring into smaller modules", lines.len()),
            });
        }
        
        let long_lines: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.len() > 120)
            .map(|(i, _)| i + 1)
            .collect();
        
        if !long_lines.is_empty() {
            issues.push(Issue {
                line: long_lines[0],
                severity: "info".to_string(),
                category: "style".to_string(),
                message: format!("Found {} lines exceeding 120 characters", long_lines.len()),
            });
        }
        
        if content.contains("password") || content.contains("secret") || content.contains("api_key") {
            issues.push(Issue {
                line: 1,
                severity: "warning".to_string(),
                category: "security".to_string(),
                message: "File contains potential secrets - ensure they are not hardcoded".to_string(),
            });
        }
    }
}

#[async_trait]
impl Tool for CodeReviewTool {
    fn name(&self) -> &str {
        "code_review"
    }

    fn description(&self) -> &str {
        "Review code files for common issues. Args: {\"path\": \"file or dir\", \"focus\": \"all|security|performance|style|documentation\"}"
    }

    async fn execute(&self, args: Value) -> Result<String, String> {
        let path = args["path"].as_str().ok_or("Missing 'path'")?;
        let path = self.validate_path(path).map_err(|e| e.to_string())?;
        let _focus = args["focus"].as_str().unwrap_or("all");
        
        if !path.exists() {
            return Err(format!("Path does not exist: {}", path.display()));
        }
        
        let mut results = Vec::new();
        let mut files_reviewed = 0;
        
        if path.is_file() {
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy().to_string();
                if self.allowed_extensions.contains(&ext) {
                    let content = tokio::fs::read_to_string(&path).await.map_err(|e| e.to_string())?;
                    let issues = self.analyze_code(&content, &ext);
                    if !issues.is_empty() {
                        results.push(FileReview {
                            path: path.display().to_string(),
                            issues,
                        });
                    }
                    files_reviewed = 1;
                }
            }
        } else if path.is_dir() {
            for entry in WalkDir::new(&path)
                .max_depth(3)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                if files_reviewed >= self.max_files_per_review {
                    break;
                }
                
                let file_path = entry.path();
                if let Some(ext) = file_path.extension() {
                    let ext = ext.to_string_lossy().to_string();
                    if self.allowed_extensions.contains(&ext) {
                        if let Ok(metadata) = tokio::fs::metadata(file_path).await {
                            if metadata.len() > self.max_file_size as u64 {
                                continue;
                            }
                        }
                        
                        if let Ok(content) = tokio::fs::read_to_string(file_path).await {
                            let issues = self.analyze_code(&content, &ext);
                            if !issues.is_empty() {
                                results.push(FileReview {
                                    path: file_path.display().to_string(),
                                    issues,
                                });
                            }
                            files_reviewed += 1;
                        }
                    }
                }
            }
        }
        
        if results.is_empty() {
            return Ok(format!(
                "Code review completed. {} file(s) reviewed. No issues found.",
                files_reviewed
            ));
        }
        
        let mut output = format!(
            "## Code Review Results\n\n{} file(s) reviewed. Found issues in {} file(s).\n\n",
            files_reviewed,
            results.len()
        );
        
        for review in results {
            output.push_str(&format!("### {}\n", review.path));
            
            let mut grouped: HashMap<String, Vec<&Issue>> = HashMap::new();
            for issue in &review.issues {
                grouped.entry(issue.category.clone()).or_default().push(issue);
            }
            
            for (category, issues) in grouped {
                output.push_str(&format!("\n**{}**:\n", category.replace("_", " ").to_uppercase()));
                for issue in issues {
                    let icon = match issue.severity.as_str() {
                        "error" => "",
                        "warning" => "",
                        _ => "",
                    };
                    output.push_str(&format!(
                        "- {} Line {}: {}\n",
                        icon, issue.line, issue.message
                    ));
                }
            }
            output.push('\n');
        }
        
        Ok(output)
    }
}

struct Issue {
    line: usize,
    severity: String,
    category: String,
    message: String,
}

struct FileReview {
    path: String,
    issues: Vec<Issue>,
}