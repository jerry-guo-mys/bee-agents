use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

use crate::llm::LlmClient;
use crate::tools::ToolExecutor;
use crate::evolution::types::{CodeAnalysis, Issue, Severity, CodeMetrics, ImprovementPlan, ImprovementType, Priority};

pub struct SelfAnalyzer {
    #[allow(dead_code)]
    llm: Arc<dyn LlmClient>,
    executor: Arc<ToolExecutor>,
    project_root: PathBuf,
}

impl SelfAnalyzer {
    pub fn new(
        llm: Arc<dyn LlmClient>,
        executor: Arc<ToolExecutor>,
        project_root: impl AsRef<Path>,
    ) -> Self {
        Self {
            llm,
            executor,
            project_root: project_root.as_ref().to_path_buf(),
        }
    }

    pub async fn analyze_codebase(&self) -> Result<Vec<CodeAnalysis>, String> {
        let mut analyses = Vec::new();

        let source_files = self.find_source_files().await?;

        for file_path in source_files.iter().take(10) {
            if let Ok(analysis) = self.analyze_file(file_path).await {
                analyses.push(analysis);
            }
        }

        Ok(analyses)
    }

    async fn find_source_files(&self) -> Result<Vec<PathBuf>, String> {
        let args = serde_json::json!({
            "pattern": "\\.rs$",
            "include": "*.rs",
            "use_regex": false,
            "path": self.project_root.to_str().unwrap()
        });

        let result = self.executor.execute("code_grep", args).await.map_err(|e| e.to_string())?;

        let mut files = Vec::new();
        for line in result.lines() {
            if line.ends_with(".rs:") {
                let file_name = line.trim_end_matches(':');
                files.push(self.project_root.join(file_name));
            }
        }

        Ok(files)
    }

    pub async fn analyze_file(&self, file_path: &Path) -> Result<CodeAnalysis, String> {
        let content = self.read_file_content(file_path).await?;

        let mut issues = Vec::new();

        issues.extend(self.analyze_syntax(&content, file_path).await?);
        issues.extend(self.analyze_structure(&content, file_path).await?);
        issues.extend(self.analyze_patterns(&content, file_path).await?);

        let metrics = self.calculate_metrics(&content);
        let overall_score = self.calculate_score(&issues, &metrics);

        Ok(CodeAnalysis {
            file_path: file_path.to_string_lossy().to_string(),
            issues,
            metrics,
            overall_score,
        })
    }

    async fn read_file_content(&self, file_path: &Path) -> Result<String, String> {
        let rel_path = file_path.strip_prefix(&self.project_root)
            .unwrap_or(file_path);
        
        let args = serde_json::json!({
            "file_path": rel_path.to_string_lossy().to_string(),
            "limit": 200
        });

        self.executor.execute("code_read", args).await.map_err(|e| e.to_string())
    }

    async fn analyze_syntax(
        &self,
        content: &str,
        _file_path: &Path,
    ) -> Result<Vec<Issue>, String> {
        let mut issues = Vec::new();

        let lines: Vec<&str> = content.lines().collect();

        for (i, line) in lines.iter().enumerate() {
            let line_num = i + 1;

            if line.contains("TODO") || line.contains("FIXME") {
                issues.push(Issue {
                    severity: Severity::Info,
                    line_number: Some(line_num),
                    description: "TODO/FIXME comment found".to_string(),
                    suggestion: "Consider implementing or removing TODO/FIXME".to_string(),
                });
            }

            if line.contains("unwrap()") || line.contains("expect(\"") {
                issues.push(Issue {
                    severity: Severity::Warning,
                    line_number: Some(line_num),
                    description: "Potential panic point".to_string(),
                    suggestion: "Consider using proper error handling with Result".to_string(),
                });
            }

            if line.contains("#[allow(") {
                issues.push(Issue {
                    severity: Severity::Info,
                    line_number: Some(line_num),
                    description: "Lint suppression found".to_string(),
                    suggestion: "Review if suppression is necessary".to_string(),
                });
            }
        }

        Ok(issues)
    }

    async fn analyze_structure(
        &self,
        content: &str,
        file_path: &Path,
    ) -> Result<Vec<Issue>, String> {
        let mut issues = Vec::new();

        if file_path.ends_with("lib.rs") || file_path.ends_with("main.rs") {
            let module_count = content.matches("pub mod").count();
            if module_count > 10 {
                issues.push(Issue {
                    severity: Severity::Warning,
                    line_number: None,
                    description: "Large module with many submodules".to_string(),
                    suggestion: "Consider splitting into multiple files".to_string(),
                });
            }
        }

        let function_count = content.matches("fn ").count();
        if function_count > 20 {
            issues.push(Issue {
                severity: Severity::Warning,
                line_number: None,
                description: "Large number of functions in single file".to_string(),
                suggestion: "Consider refactoring into smaller units".to_string(),
            });
        }

        Ok(issues)
    }

    async fn analyze_patterns(
        &self,
        content: &str,
        _file_path: &Path,
    ) -> Result<Vec<Issue>, String> {
        let mut issues = Vec::new();

        if content.contains("match ") && !content.contains("_ =>") {
            issues.push(Issue {
                severity: Severity::Warning,
                line_number: None,
                description: "Match statement may not handle all cases".to_string(),
                suggestion: "Add default case or ensure exhaustiveness".to_string(),
            });
        }

        if content.contains(".clone()") && content.contains(".clone()") {
            let clone_count = content.matches(".clone()").count();
            if clone_count > 5 {
                issues.push(Issue {
                    severity: Severity::Info,
                    line_number: None,
                    description: "Multiple clone() calls".to_string(),
                    suggestion: "Consider using references or smarter ownership".to_string(),
                });
            }
        }

        Ok(issues)
    }

    fn calculate_metrics(&self, content: &str) -> CodeMetrics {
        let lines_of_code = content.lines().count();

        let comment_count = content.lines()
            .filter(|l| l.trim().starts_with("//"))
            .count();
        
        let documentation_coverage = if lines_of_code > 0 {
            Some(comment_count as f64 / lines_of_code as f64)
        } else {
            None
        };

        CodeMetrics {
            lines_of_code,
            cyclomatic_complexity: None,
            documentation_coverage,
            test_coverage: None,
        }
    }

    fn calculate_score(&self, issues: &[Issue], metrics: &CodeMetrics) -> f64 {
        let mut score: f64 = 1.0;

        for issue in issues {
            score *= match issue.severity {
                Severity::Error => 0.7,
                Severity::Warning => 0.9,
                Severity::Info => 0.95,
            };
        }

        if let Some(doc_coverage) = metrics.documentation_coverage {
            if doc_coverage < 0.1 {
                score *= 0.9;
            }
        }

        score.clamp(0.0, 1.0)
    }

    pub async fn generate_improvement_plans(
        &self,
        analyses: &[CodeAnalysis],
    ) -> Result<Vec<ImprovementPlan>, String> {
        let mut plans = Vec::new();

        for analysis in analyses {
            if analysis.overall_score < 0.8 && !analysis.issues.is_empty() {
                let plan = self.create_plan_from_analysis(analysis).await?;
                plans.push(plan);
            }
        }

        plans.sort_by(|a, b| b.priority.cmp(&a.priority));

        Ok(plans)
    }

    async fn create_plan_from_analysis(
        &self,
        analysis: &CodeAnalysis,
    ) -> Result<ImprovementPlan, String> {
        let mut plan = ImprovementPlan {
            id: Uuid::new_v4().to_string(),
            title: String::new(),
            description: String::new(),
            target_files: vec![analysis.file_path.clone()],
            improvement_type: ImprovementType::Refactor,
            expected_outcome: String::new(),
            priority: Priority::Medium,
        };

        let error_count = analysis.issues.iter()
            .filter(|i| matches!(i.severity, Severity::Error))
            .count();
        let warning_count = analysis.issues.iter()
            .filter(|i| matches!(i.severity, Severity::Warning))
            .count();

        if error_count > 0 {
            plan.improvement_type = ImprovementType::BugFix;
            plan.priority = Priority::Critical;
            plan.title = format!("Fix {} errors in {}", error_count, analysis.file_path);
            plan.description = "Address critical errors in code".to_string();
        } else if warning_count > 3 {
            plan.improvement_type = ImprovementType::Refactor;
            plan.priority = Priority::High;
            plan.title = format!("Refactor {} ({} warnings)", analysis.file_path, warning_count);
            plan.description = "Improve code quality by addressing warnings".to_string();
        } else if let Some(doc_coverage) = analysis.metrics.documentation_coverage {
            if doc_coverage < 0.1 {
                plan.improvement_type = ImprovementType::Documentation;
                plan.priority = Priority::Low;
                plan.title = format!("Add documentation to {}", analysis.file_path);
                plan.description = "Improve documentation coverage".to_string();
            }
        }

        if plan.title.is_empty() {
            plan.title = format!("Improve {}", analysis.file_path);
            plan.description = "General code improvements".to_string();
        }

        plan.expected_outcome = format!(
            "Improve quality score from {:.2} to >= 0.8",
            analysis.overall_score
        );

        Ok(plan)
    }
}
