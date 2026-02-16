pub mod analyzer;
pub mod engine;
pub mod executor;
pub mod planner;
pub mod loop_;
pub mod types;

pub use analyzer::SelfAnalyzer;
pub use engine::{EvolutionEngine, EvolutionConfig};
pub use executor::ExecutionEngine;
pub use planner::ImprovementPlanner;
pub use loop_::EvolutionLoop;
pub use types::{
    ImprovementPlan, ImprovementType, Priority,
    CodeAnalysis, Issue, Severity, CodeMetrics,
    IterationResult,
};
