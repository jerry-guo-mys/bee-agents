//! 工作流构建器
//!
//! 提供流畅的API来构建工作流

use std::collections::HashMap;
#[cfg(feature = "gateway")]
use crate::gateway::BackgroundTask;
use crate::workflow::types::*;

/// 工作流构建器
pub struct WorkflowBuilder {
    id: WorkflowId,
    name: String,
    description: Option<String>,
    user_id: String,
    session_id: Option<String>,
    tasks: HashMap<TaskId, WorkflowTask>,
}

impl WorkflowBuilder {
    /// 创建新的工作流构建器
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: format!("wf_{}", uuid::Uuid::new_v4()),
            name: name.into(),
            description: None,
            user_id: String::new(),
            session_id: None,
            tasks: HashMap::new(),
        }
    }

    /// 设置描述
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// 设置用户ID
    pub fn user_id(mut self, user_id: String) -> Self {
        self.user_id = user_id;
        self
    }

    /// 设置会话ID
    pub fn session_id(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// 添加任务
    #[cfg(feature = "gateway")]
    pub fn task(mut self, id: impl Into<TaskId>, task: BackgroundTask) -> Self {
        let id = id.into();
        self.tasks.insert(id.clone(), WorkflowTask {
            id,
            definition: TaskDefinition::Simple(Box::new(task)),
            dependencies: TaskDependencies::None,
            fallback: None,
            state: TaskState::Waiting,
        });
        self
    }

    /// 设置顺序依赖
    pub fn sequential(mut self, from: impl Into<TaskId>, to: impl Into<TaskId>) -> Self {
        let to_id = to.into();
        if let Some(task) = self.tasks.get_mut(&to_id) {
            task.dependencies = TaskDependencies::Sequential(from.into());
        }
        self
    }

    /// 设置AND依赖（所有前置任务）
    pub fn depends_on_all(mut self, task_id: impl Into<TaskId>, deps: Vec<TaskId>) -> Self {
        let id = task_id.into();
        if let Some(task) = self.tasks.get_mut(&id) {
            task.dependencies = TaskDependencies::All(deps);
        }
        self
    }

    /// 设置OR依赖（任一前置任务）
    pub fn depends_on_any(mut self, task_id: impl Into<TaskId>, deps: Vec<TaskId>) -> Self {
        let id = task_id.into();
        if let Some(task) = self.tasks.get_mut(&id) {
            task.dependencies = TaskDependencies::Any(deps);
        }
        self
    }

    /// 设置失败备用任务
    pub fn with_fallback(mut self, task_id: impl Into<TaskId>, fallback_id: TaskId) -> Self {
        let id = task_id.into();
        if let Some(task) = self.tasks.get_mut(&id) {
            task.fallback = Some(fallback_id);
        }
        self
    }

    /// 构建工作流
    pub fn build(self) -> Result<Workflow, WorkflowError> {
        if self.user_id.is_empty() {
            return Err(WorkflowError::InvalidConfiguration("user_id is required".to_string()));
        }

        Ok(Workflow {
            id: self.id,
            name: self.name,
            description: self.description,
            user_id: self.user_id,
            session_id: self.session_id,
            tasks: self.tasks,
            status: WorkflowStatus::Created,
            created_at: chrono::Utc::now().timestamp_millis(),
            started_at: None,
            completed_at: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "gateway")]
    use crate::gateway::BackgroundTask;

    #[cfg(feature = "gateway")]
    #[test]
    fn test_build_simple_workflow() {
        let workflow = WorkflowBuilder::new("Test Workflow")
            .description("A test workflow")
            .user_id("user1".to_string())
            .task("task1", BackgroundTask::new("user1".to_string(), "Do something".to_string()))
            .task("task2", BackgroundTask::new("user1".to_string(), "Do next".to_string()))
            .sequential("task1", "task2")
            .build()
            .expect("Failed to build workflow");

        assert_eq!(workflow.name, "Test Workflow");
        assert_eq!(workflow.tasks.len(), 2);
        assert!(matches!(workflow.tasks.get("task2").unwrap().dependencies, TaskDependencies::Sequential(_)));
    }

    #[test]
    fn test_build_without_user_id_fails() {
        let result = WorkflowBuilder::new("Test")
            .build();
        
        assert!(result.is_err());
    }
}
