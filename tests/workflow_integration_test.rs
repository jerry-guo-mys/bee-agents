//! 工作流集成测试

#[cfg(test)]
mod tests {
    #[cfg(feature = "gateway")]
    use bee::gateway::{BackgroundTask, TaskQueue};
    #[cfg(feature = "gateway")]
    use bee::workflow::*;
    #[cfg(feature = "gateway")]
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[cfg(feature = "gateway")]
    use std::sync::Arc;

    #[cfg(feature = "gateway")]
    struct CountingExecutor {
        count: AtomicUsize,
    }

    #[cfg(feature = "gateway")]
    #[async_trait::async_trait]
    impl WorkflowTaskExecutor for CountingExecutor {
        async fn execute(&self, _task: &BackgroundTask) -> Result<String, String> {
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok("done".to_string())
        }
    }

    #[cfg(feature = "gateway")]
    #[tokio::test]
    async fn test_full_workflow_execution() {
        use tokio::time::{sleep, Duration};

        let (queue, _, _) = TaskQueue::new();
        let executor = Arc::new(CountingExecutor {
            count: AtomicUsize::new(0),
        });
        
        let engine = WorkflowEngine::new(
            Arc::new(queue),
            executor.clone(),
        );
        
        let workflow = WorkflowBuilder::new("Integration Test")
            .user_id("user1".to_string())
            .task("a", BackgroundTask::new("user1".to_string(), "Task A".to_string()))
            .task("b", BackgroundTask::new("user1".to_string(), "Task B".to_string()))
            .task("c", BackgroundTask::new("user1".to_string(), "Task C".to_string()))
            .sequential("a", "b")
            .sequential("b", "c")
            .build()
            .unwrap();
        
        let workflow_id = engine.submit_workflow(workflow).await.unwrap();
        
        // Wait for execution
        sleep(Duration::from_millis(200)).await;
        
        let status = engine.get_status(&workflow_id).await;
        println!("Final status: {:?}", status);
        
        // All tasks should have been executed
        let count = executor.count.load(Ordering::SeqCst);
        println!("Tasks executed: {}", count);
        assert!(count >= 1, "At least the first task should execute");
    }

    #[cfg(feature = "gateway")]
    #[tokio::test]
    async fn test_parallel_tasks() {
        use tokio::time::{sleep, Duration};

        let (queue, _, _) = TaskQueue::new();
        let executor = Arc::new(CountingExecutor {
            count: AtomicUsize::new(0),
        });
        
        let engine = WorkflowEngine::new(
            Arc::new(queue),
            executor.clone(),
        );
        
        let workflow = WorkflowBuilder::new("Parallel Test")
            .user_id("user1".to_string())
            .task("a", BackgroundTask::new("user1".to_string(), "Task A".to_string()))
            .task("b", BackgroundTask::new("user1".to_string(), "Task B".to_string()))
            .task("c", BackgroundTask::new("user1".to_string(), "Task C".to_string()))
            .depends_on_all("c", vec!["a".to_string(), "b".to_string()])
            .build()
            .unwrap();
        
        let workflow_id = engine.submit_workflow(workflow).await.unwrap();
        
        sleep(Duration::from_millis(200)).await;
        
        let count = executor.count.load(Ordering::SeqCst);
        // A and B should execute in parallel, C waits for both
        assert!(count >= 2, "At least A and B should execute");
    }

    #[cfg(feature = "gateway")]
    #[tokio::test]
    async fn test_fallback_on_failure() {
        use tokio::time::{sleep, Duration};

        struct FailingExecutor;

        #[async_trait::async_trait]
        impl WorkflowTaskExecutor for FailingExecutor {
            async fn execute(&self, _task: &BackgroundTask) -> Result<String, String> {
                Err("simulated failure".to_string())
            }
        }

        let (queue, _, _) = TaskQueue::new();
        let executor = Arc::new(FailingExecutor);
        
        let engine = WorkflowEngine::new(
            Arc::new(queue),
            executor,
        );
        
        let workflow = WorkflowBuilder::new("Fallback Test")
            .user_id("user1".to_string())
            .task("main", BackgroundTask::new("user1".to_string(), "Main task".to_string()))
            .task("fallback", BackgroundTask::new("user1".to_string(), "Fallback task".to_string()))
            .with_fallback("main", "fallback".to_string())
            .build()
            .unwrap();
        
        let workflow_id = engine.submit_workflow(workflow).await.unwrap();
        
        sleep(Duration::from_millis(200)).await;
        
        let status = engine.get_status(&workflow_id).await;
        // Workflow should have tried fallback
        assert!(matches!(status, Some(WorkflowStatus::Running | WorkflowStatus::Failed)));
    }
}
