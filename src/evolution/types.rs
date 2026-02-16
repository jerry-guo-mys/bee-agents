use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementPlan {
    pub id: String,
    pub title: String,
    pub description: String,
    pub target_files: Vec<String>,
    pub improvement_type: ImprovementType,
    pub expected_outcome: String,
    pub priority: Priority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImprovementType {
    BugFix,
    Performance,
    Refactor,
    Feature,
    Documentation,
    Test,
}

impl std::fmt::Display for ImprovementType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImprovementType::BugFix => write!(f, "BugFix"),
            ImprovementType::Performance => write!(f, "Performance"),
            ImprovementType::Refactor => write!(f, "Refactor"),
            ImprovementType::Feature => write!(f, "Feature"),
            ImprovementType::Documentation => write!(f, "Documentation"),
            ImprovementType::Test => write!(f, "Test"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysis {
    pub file_path: String,
    pub issues: Vec<Issue>,
    pub metrics: CodeMetrics,
    pub overall_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub severity: Severity,
    pub line_number: Option<usize>,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeMetrics {
    pub lines_of_code: usize,
    pub cyclomatic_complexity: Option<f64>,
    pub documentation_coverage: Option<f64>,
    pub test_coverage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationResult {
    pub iteration: usize,
    pub success: bool,
    pub changes_made: Vec<String>,
    pub tests_passed: bool,
    pub quality_score: f64,
    pub lessons_learned: Vec<String>,
}
