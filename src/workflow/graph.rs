//! 工作流依赖图
//!
//! 使用邻接表和入度表实现 DAG 拓扑排序

use std::collections::HashMap;
use crate::workflow::types::*;

/// 工作流依赖图
pub struct WorkflowGraph {
    /// 邻接表：任务 ID -> 依赖该任务的任务列表
    pub adjacency: HashMap<TaskId, Vec<TaskId>>,
    /// 入度表：任务 ID -> 未完成的依赖数
    pub in_degree: HashMap<TaskId, usize>,
}

impl WorkflowGraph {
    /// 创建依赖图
    pub fn new(tasks: &HashMap<TaskId, WorkflowTask>) -> Self {
        let mut adjacency: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
        let mut in_degree: HashMap<TaskId, usize> = HashMap::new();

        for task_id in tasks.keys() {
            in_degree.insert(task_id.clone(), 0);
            adjacency.insert(task_id.clone(), Vec::new());
        }

        for (task_id, task) in tasks {
            match &task.dependencies {
                TaskDependencies::None => {}
                TaskDependencies::Sequential(dep_id) => {
                    adjacency.entry(dep_id.clone()).or_default().push(task_id.clone());
                    *in_degree.entry(task_id.clone()).or_insert(0) += 1;
                }
                TaskDependencies::All(dep_ids) => {
                    for dep_id in dep_ids {
                        adjacency.entry(dep_id.clone()).or_default().push(task_id.clone());
                        *in_degree.entry(task_id.clone()).or_insert(0) += 1;
                    }
                }
                TaskDependencies::Any(dep_ids) => {
                    for dep_id in dep_ids {
                        adjacency.entry(dep_id.clone()).or_default().push(task_id.clone());
                    }
                    in_degree.insert(task_id.clone(), 1);
                }
                TaskDependencies::Condition { task_id: dep_id, .. } => {
                    adjacency.entry(dep_id.clone()).or_default().push(task_id.clone());
                    *in_degree.entry(task_id.clone()).or_insert(0) += 1;
                }
            }
        }

        Self { adjacency, in_degree }
    }

    /// 获取可执行的任务（入度为 0 且未执行）
    pub fn get_ready_tasks(&self, states: &HashMap<TaskId, TaskState>) -> Vec<TaskId> {
        self.in_degree
            .iter()
            .filter(|(task_id, degree)| {
                **degree == 0 && matches!(states.get(*task_id), Some(TaskState::Waiting) | None)
            })
            .map(|(task_id, _)| task_id.clone())
            .collect()
    }

    /// 更新任务完成状态，返回新变为可执行的任务
    pub fn mark_completed(
        &mut self,
        completed_task_id: &TaskId,
        _tasks: &HashMap<TaskId, WorkflowTask>,
        completed_task_state: TaskState,
    ) -> Vec<(TaskId, bool)> {
        let mut newly_ready = Vec::new();

        if let Some(dependents) = self.adjacency.get(completed_task_id) {
            for dependent_id in dependents {
                if let Some(degree) = self.in_degree.get_mut(dependent_id) {
                    if let Some(task) = _tasks.get(dependent_id) {
                        match &task.dependencies {
                            TaskDependencies::Any(_) => {
                                if completed_task_state == TaskState::Completed {
                                    *degree = 0;
                                }
                            }
                            TaskDependencies::Condition { .. } => {
                                let condition_met = completed_task_state == TaskState::Completed;
                                *degree -= 1;
                                if *degree == 0 {
                                    newly_ready.push((dependent_id.clone(), condition_met));
                                    continue;
                                }
                            }
                            _ => {
                                *degree -= 1;
                            }
                        }
                    }

                    if *degree == 0 {
                        newly_ready.push((dependent_id.clone(), true));
                    }
                }
            }
        }

        newly_ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "gateway")]
    use crate::gateway::BackgroundTask;

    #[cfg(feature = "gateway")]
    fn create_test_task(id: &str, deps: TaskDependencies) -> WorkflowTask {
        WorkflowTask {
            id: id.to_string(),
            definition: TaskDefinition::Simple(Box::new(BackgroundTask::new(
                "user1".to_string(),
                format!("Task {}", id),
            ))),
            dependencies: deps,
            fallback: None,
            state: TaskState::Waiting,
        }
    }

    #[cfg(feature = "gateway")]
    #[test]
    fn test_graph_construction_sequential() {
        let mut tasks = HashMap::new();
        tasks.insert("task1".to_string(), create_test_task("task1", TaskDependencies::None));
        tasks.insert("task2".to_string(), create_test_task("task2", TaskDependencies::Sequential("task1".to_string())));
        
        let graph = WorkflowGraph::new(&tasks);
        
        assert_eq!(graph.in_degree.get("task1"), Some(&0));
        assert_eq!(graph.in_degree.get("task2"), Some(&1));
    }

    #[cfg(feature = "gateway")]
    #[test]
    fn test_get_ready_tasks() {
        let mut tasks = HashMap::new();
        tasks.insert("task1".to_string(), create_test_task("task1", TaskDependencies::None));
        tasks.insert("task2".to_string(), create_test_task("task2", TaskDependencies::Sequential("task1".to_string())));
        
        let graph = WorkflowGraph::new(&tasks);
        let states: HashMap<TaskId, TaskState> = tasks.iter()
            .map(|(k, v)| (k.clone(), v.state))
            .collect();
        
        let ready = graph.get_ready_tasks(&states);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0], "task1");
    }
}
