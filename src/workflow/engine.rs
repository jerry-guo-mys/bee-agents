//! 工作流引擎
//!
//! 核心执行引擎，管理工作流生命周期和任务调度

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use async_trait::async_trait;

#[cfg(feature = "gateway")]
use crate::gateway::{BackgroundTask, TaskQueue};
use crate::workflow::types::*;
use crate::workflow::graph::WorkflowGraph;

/// 工作流任务执行器 trait
#[async_trait]
pub trait WorkflowTaskExecutor: Send + Sync {
    /// 执行单个任务
    async fn execute(&self, task: &BackgroundTask) -> Result<String, String>;
}

/// 工作流引擎
pub struct WorkflowEngine {
    #[cfg(feature = "gateway")]
    task_queue: Arc<TaskQueue>,
    workflows: RwLock<HashMap<WorkflowId, Workflow>>,
    executor: Arc<dyn WorkflowTaskExecutor>,
}

#[cfg(feature = "gateway")]
impl WorkflowEngine {
    /// 创建新的工作流引擎
    pub fn new(
        task_queue: Arc<TaskQueue>,
        executor: Arc<dyn WorkflowTaskExecutor>,
    ) -> Self {
        Self {
            task_queue,
            workflows: RwLock::new(HashMap::new()),
            executor,
        }
    }

    /// 提交工作流
    pub async fn submit_workflow(&self, workflow: Workflow) -> Result<WorkflowId, WorkflowError> {
        let workflow_id = workflow.id.clone();
        
        self.workflows.write().await.insert(workflow_id.clone(), workflow);
        
        self.start_workflow(&workflow_id).await?;
        
        Ok(workflow_id)
    }

    /// 启动工作流执行
    async fn start_workflow(&self, workflow_id: &WorkflowId) -> Result<(), WorkflowError> {
        let mut workflows = self.workflows.write().await;
        let workflow = workflows.get_mut(workflow_id)
            .ok_or(WorkflowError::WorkflowNotFound)?;
        
        workflow.status = WorkflowStatus::Running;
        workflow.started_at = Some(chrono::Utc::now().timestamp_millis());
        
        let graph = WorkflowGraph::new(&workflow.tasks);
        let states: HashMap<_, _> = workflow.tasks.iter()
            .map(|(k, v)| (k.clone(), v.state))
            .collect();
        
        let ready_tasks = graph.get_ready_tasks(&states);
        
        drop(workflows);
        
        for task_id in ready_tasks {
            self.submit_task(workflow_id, &task_id).await?;
        }
        
        Ok(())
    }

    /// 提交单个任务到队列
    async fn submit_task(
        &self,
        workflow_id: &WorkflowId,
        task_id: &TaskId,
    ) -> Result<(), WorkflowError> {
        let mut workflows = self.workflows.write().await;
        let workflow = workflows.get_mut(workflow_id)
            .ok_or(WorkflowError::WorkflowNotFound)?;
        
        let task = workflow.tasks.get_mut(task_id)
            .ok_or(WorkflowError::TaskNotFound)?;
        
        task.state = TaskState::Running;
        
        if let TaskDefinition::Simple(bg_task) = &task.definition {
            let bg_task = bg_task.clone();
            let _workflow_id = workflow_id.clone();
            let _task_id = task_id.clone();
            let queue = Arc::clone(&self.task_queue);
            let executor = Arc::clone(&self.executor);
            
            tokio::spawn(async move {
                let wrapper = BackgroundTask::new(
                    bg_task.user_id.clone(),
                    bg_task.instruction.clone(),
                );
                let submitted_id = queue.submit(wrapper).await;
                
                match executor.execute(&bg_task).await {
                    Ok(result) => {
                        queue.set_result(&submitted_id, result).await;
                    }
                    Err(error) => {
                        queue.set_error(&submitted_id, error).await;
                    }
                }
            });
        }
        
        Ok(())
    }

    /// 获取工作流状态
    pub async fn get_status(&self, workflow_id: &WorkflowId) -> Option<WorkflowStatus> {
        self.workflows.read().await
            .get(workflow_id)
            .map(|w| w.status)
    }

    /// 处理任务完成回调
    pub async fn on_task_completed(
        &self,
        workflow_id: &WorkflowId,
        task_id: &TaskId,
        result: Result<String, String>,
    ) -> Result<(), WorkflowError> {
        let mut workflows = self.workflows.write().await;
        let workflow = workflows.get_mut(workflow_id)
            .ok_or(WorkflowError::WorkflowNotFound)?;
        
        let task = workflow.tasks.get_mut(task_id)
            .ok_or(WorkflowError::TaskNotFound)?;
        
        match result {
            Ok(_) => {
                task.state = TaskState::Completed;
            }
            Err(_) => {
                task.state = TaskState::Failed;
                
                if let Some(fallback_id) = task.fallback.clone() {
                    drop(workflows);
                    self.submit_task(workflow_id, &fallback_id).await?;
                    return Ok(());
                }
            }
        }
        
        drop(workflows);
        
        self.check_completion(workflow_id).await;
        
        Ok(())
    }

    async fn check_completion(&self, workflow_id: &WorkflowId) {
        let mut workflows = self.workflows.write().await;
        if let Some(workflow) = workflows.get_mut(workflow_id) {
            let all_finished = workflow.tasks.values().all(|task| {
                matches!(task.state, TaskState::Completed | TaskState::Failed | TaskState::Skipped)
            });
            
            if all_finished {
                let all_success = workflow.tasks.values().all(|task| {
                    matches!(task.state, TaskState::Completed)
                });
                
                workflow.status = if all_success {
                    WorkflowStatus::Completed
                } else {
                    WorkflowStatus::Failed
                };
                workflow.completed_at = Some(chrono::Utc::now().timestamp_millis());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::WorkflowBuilder;
    #[cfg(feature = "gateway")]
    use crate::gateway::{BackgroundTask, TaskQueue};
    use std::sync::Arc;

    struct MockExecutor;

    #[async_trait]
    impl WorkflowTaskExecutor for MockExecutor {
        async fn execute(&self, _task: &BackgroundTask) -> Result<String, String> {
            Ok("success".to_string())
        }
    }

    #[cfg(feature = "gateway")]
    #[tokio::test]
    async fn test_submit_workflow() {
        let (queue, _, _) = TaskQueue::new();
        let engine = WorkflowEngine::new(
            Arc::new(queue),
            Arc::new(MockExecutor),
        );

        let workflow = WorkflowBuilder::new("Test")
            .user_id("user1".to_string())
            .task("task1", BackgroundTask::new("user1".to_string(), "Do something".to_string()))
            .build()
            .unwrap();

        let workflow_id = engine.submit_workflow(workflow).await.unwrap();
        assert!(!workflow_id.is_empty());
    }

    #[cfg(feature = "gateway")]
    #[tokio::test]
    async fn test_sequential_execution() {
        let (queue, _, _) = TaskQueue::new();
        let engine = Arc::new(WorkflowEngine::new(
            Arc::new(queue),
            Arc::new(MockExecutor),
        ));

        let workflow = WorkflowBuilder::new("Sequential Test")
            .user_id("user1".to_string())
            .task("task1", BackgroundTask::new("user1".to_string(), "Step 1".to_string()))
            .task("task2", BackgroundTask::new("user1".to_string(), "Step 2".to_string()))
            .sequential("task1", "task2")
            .build()
            .unwrap();

        let workflow_id = engine.submit_workflow(workflow).await.unwrap();
        
        let status = engine.get_status(&workflow_id).await;
        assert!(matches!(status, Some(WorkflowStatus::Running)));
    }
}
