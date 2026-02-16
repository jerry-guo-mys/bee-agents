use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::llm::LlmClient;
use crate::tools::ToolExecutor;
use crate::evolution::analyzer::SelfAnalyzer;
use crate::evolution::planner::ImprovementPlanner;
use crate::evolution::executor::ExecutionEngine;
use crate::evolution::engine::{EvolutionEngine, EvolutionConfig};
use crate::evolution::types::{ImprovementPlan, IterationResult};

pub struct EvolutionLoop {
    engine: EvolutionEngine,
    analyzer: SelfAnalyzer,
    planner: ImprovementPlanner,
    executor: ExecutionEngine,
}

impl EvolutionLoop {
    pub fn new(
        llm: Arc<dyn LlmClient>,
        executor: Arc<ToolExecutor>,
        config: EvolutionConfig,
        project_root: PathBuf,
    ) -> Self {
        let analyzer = SelfAnalyzer::new(llm.clone(), executor.clone(), &project_root);
        let planner = ImprovementPlanner::new(llm.clone(), executor.clone());

        Self {
            engine: EvolutionEngine::new(config.clone()),
            analyzer,
            planner,
            executor: ExecutionEngine::new(executor, project_root, config),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.engine.is_enabled()
    }

    pub async fn run(&mut self) -> Result<Vec<IterationResult>, String> {
        let mut results = Vec::new();

        while self.engine.can_continue() {
            let iteration = self.engine.current_iteration() + 1;
            println!("Starting evolution iteration {}", iteration);

            match self.run_iteration().await {
                Ok(result) => {
                    let mut result = result;
                    result.iteration = iteration;
                    results.push(result.clone());

                    if result.success && result.quality_score >= self.engine.config().target_score_threshold {
                        println!("Iteration {} succeeded with quality score: {:.2}", iteration, result.quality_score);
                        if result.lessons_learned.is_empty() {
                            break;
                        }
                    } else {
                        println!("Iteration {} failed or needs improvement", iteration);
                    }
                }
                Err(e) => {
                    println!("Iteration {} failed with error: {}", iteration, e);
                    results.push(IterationResult {
                        iteration,
                        success: false,
                        changes_made: vec![],
                        tests_passed: false,
                        quality_score: 0.0,
                        lessons_learned: vec![e],
                    });
                }
            }

            self.engine.increment_iteration();
        }

        Ok(results)
    }

    async fn run_iteration(&self) -> Result<IterationResult, String> {
        let analyses = self.analyzer.analyze_codebase().await?;
        
        if analyses.is_empty() {
            return Ok(IterationResult {
                iteration: 0,
                success: true,
                changes_made: vec![],
                tests_passed: true,
                quality_score: 1.0,
                lessons_learned: vec![],
            });
        }

        let plans = self.analyzer.generate_improvement_plans(&analyses).await?;
        
        if plans.is_empty() {
            return Ok(IterationResult {
                iteration: 0,
                success: true,
                changes_made: vec![],
                tests_passed: true,
                quality_score: 0.9,
                lessons_learned: vec!["No improvement plans generated".to_string()],
            });
        }

        let first_plan = &plans[0];
        let target_analysis = analyses.iter()
            .find(|a| a.file_path == first_plan.target_files[0])
            .ok_or("No matching analysis found")?;

        let steps = self.planner.plan_improvements(target_analysis, first_plan).await?;

        let refined_steps = self.planner.refine_steps_with_context(&steps, "Self-improvement iteration").await?;

        let result = self.executor.execute_plan(first_plan, &refined_steps).await?;

        Ok(result)
    }

    pub async fn run_targeted_iteration(
        &mut self,
        target_files: Vec<String>,
        goal: &str,
    ) -> Result<IterationResult, String> {
        let iteration = self.engine.current_iteration() + 1;
        println!("Starting targeted iteration {} for goal: {}", iteration, goal);

        let mut analyses = Vec::new();
        for file_path in &target_files {
            if let Ok(analysis) = self.analyzer.analyze_file(Path::new(file_path)).await {
                analyses.push(analysis);
            }
        }

        if analyses.is_empty() {
            return Err("No valid analyses for target files".to_string());
        }

        let plan = ImprovementPlan {
            id: uuid::Uuid::new_v4().to_string(),
            title: format!("Targeted improvement: {}", goal),
            description: goal.to_string(),
            target_files,
            improvement_type: crate::evolution::types::ImprovementType::Refactor,
            expected_outcome: format!("Achieve goal: {}", goal),
            priority: crate::evolution::types::Priority::High,
        };

        let first_analysis = &analyses[0];
        let steps = self.planner.plan_improvements(first_analysis, &plan).await?;

        let refined_steps = self.planner.refine_steps_with_context(&steps, goal).await?;

        let result = self.executor.execute_plan(&plan, &refined_steps).await?;

        let mut result = result;
        result.iteration = iteration;
        self.engine.increment_iteration();

        Ok(result)
    }
}
