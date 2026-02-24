# Task Workflow Dependency Design

## Overview

本文档描述了Bee项目中任务工作流（Workflow）依赖系统的设计方案，支持复杂的任务编排模式，包括依赖链、并行执行、条件分支和嵌套工作流。

## Goals

1. **支持复杂依赖关系**：AND、OR、条件依赖、顺序依赖
2. **支持嵌套工作流**：工作流可以包含子工作流，实现分层任务管理
3. **失败备用路径**：任务失败时可以切换到预定义的替代路径
4. **AI研究场景优化**：特别适合多源搜索→验证→整合→报告的流程
5. **与现有系统兼容**：复用现有的`BackgroundTask`和`TaskQueue`

## Non-Goals

1. 分布式任务执行（当前版本只支持单节点）
2. 跨会话任务依赖
3. 循环依赖检测（假设用户不会创建循环）

## Architecture

### Core Components

```
┌─────────────────────────────────────────────────────────────────┐
│                     WorkflowEngine                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │   Parser     │  │  Scheduler   │  │   State Manager      │  │
│  │ (DSL/JSON)   │  │ (Topology)   │  │ (Persistence)        │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────┘  │
│         │                 │                     │              │
│         └─────────────────┼─────────────────────┘              │
│                           ▼                                    │
│                  ┌─────────────────┐                          │
│                  │  TaskExecutor   │                          │
│                  │ (Integration)   │                          │
│                  └────────┬────────┘                          │
│                           │                                    │
└───────────────────────────┼────────────────────────────────────┘
                            ▼
                  ┌─────────────────┐
                  │   TaskQueue     │  (Existing)
                  │  (Background)   │
                  └─────────────────┘
```

### Data Models

#### Workflow

```rust
/// 工作流定义
pub struct Workflow {
    /// 工作流唯一标识
    pub id: WorkflowId,
    /// 工作流名称
    pub name: String,
    /// 工作流描述
    pub description: Option<String>,
    /// 所属用户
    pub user_id: String,
    /// 关联会话
    pub session_id: Option<String>,
    /// 工作流中的所有任务
    pub tasks: HashMap<TaskId, WorkflowTask>,
    /// 依赖关系图
    pub graph: WorkflowGraph,
    /// 当前状态
    pub status: WorkflowStatus,
    /// 创建时间
    pub created_at: i64,
    /// 开始执行时间
    pub started_at: Option<i64>,
    /// 完成时间
    pub completed_at: Option<i64>,
    /// 元数据
    pub metadata: Option<serde_json::Value>,
}

pub type WorkflowId = String;

/// 工作流状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    /// 已创建，等待执行
    Created,
    /// 正在执行
    Running,
    /// 已完成
    Completed,
    /// 执行失败
    Failed,
    /// 已取消
    Cancelled,
    /// 已暂停
    Paused,
}
```

#### WorkflowTask

```rust
/// 工作流中的任务节点
pub struct WorkflowTask {
    /// 任务ID
    pub id: TaskId,
    /// 任务定义
    pub definition: TaskDefinition,
    /// 依赖配置
    pub dependencies: TaskDependencies,
    /// 失败时的备用任务ID
    pub fallback: Option<TaskId>,
    /// 重试策略
    pub retry_policy: RetryPolicy,
    /// 执行状态
    pub state: TaskState,
}

/// 任务定义
pub enum TaskDefinition {
    /// 简单任务：复用现有的BackgroundTask
    Simple(BackgroundTask),
    /// 子工作流：嵌套另一个工作流
    SubWorkflow(Box<Workflow>),
    /// 并行任务组：Map模式
    Parallel(Vec<BackgroundTask>),
}

/// 任务依赖类型
pub enum TaskDependencies {
    /// 无依赖，可立即执行
    None,
    /// 顺序依赖：指定任务完成后执行
    Sequential(TaskId),
    /// AND依赖：所有指定任务都完成
    All(Vec<TaskId>),
    /// OR依赖：任一指定任务完成
    Any(Vec<TaskId>),
    /// 条件依赖：前置任务满足条件
    Condition {
        task_id: TaskId,
        predicate: ConditionPredicate,
    },
}

/// 条件谓词（可序列化的条件定义）
#[derive(Clone, Serialize, Deserialize)]
pub enum ConditionPredicate {
    /// 任务成功完成
    Success,
    /// 任务返回结果包含指定文本
    ResultContains(String),
    /// 自定义条件（运行时解析）
    Custom(String), // 存储条件表达式，如 "result.len() > 100"
}

/// 任务状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskState {
    /// 等待依赖满足
    Waiting,
    /// 依赖已满足，准备执行
    Ready,
    /// 已提交到队列
    Pending,
    /// 正在执行
    Running,
    /// 已完成
    Completed,
    /// 失败
    Failed,
    /// 跳过（条件不满足）
    Skipped,
}

/// 重试策略
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// 最大重试次数
    pub max_retries: u32,
    /// 重试间隔（毫秒）
    pub retry_interval_ms: u64,
    /// 是否指数退避
    pub exponential_backoff: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 0,
            retry_interval_ms: 1000,
            exponential_backoff: false,
        }
    }
}
```

#### WorkflowGraph

```rust
/// 工作流依赖图
pub struct WorkflowGraph {
    /// 邻接表：任务ID -> 依赖该任务的任务列表
    adjacency: HashMap<TaskId, Vec<TaskId>>,
    /// 入度表：任务ID -> 未完成的依赖数
    in_degree: HashMap<TaskId, usize>,
}

impl WorkflowGraph {
    /// 创建依赖图
    pub fn new(tasks: &HashMap<TaskId, WorkflowTask>) -> Self {
        let mut adjacency: HashMap<TaskId, Vec<TaskId>> = HashMap::new();
        let mut in_degree: HashMap<TaskId, usize> = HashMap::new();

        // 初始化所有任务的入度为0
        for task_id in tasks.keys() {
            in_degree.insert(task_id.clone(), 0);
            adjacency.insert(task_id.clone(), Vec::new());
        }

        // 构建依赖关系
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
                    // OR依赖：任一完成即可，使用特殊处理
                    // 实际入度为1，但需要跟踪哪些依赖已完成
                    for dep_id in dep_ids {
                        adjacency.entry(dep_id.clone()).or_default().push(task_id.clone());
                    }
                    // OR依赖初始入度为1，表示只需要一个完成
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

    /// 获取可执行的任务（入度为0且未执行）
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
        states: &HashMap<TaskId, TaskState>,
    ) -> Vec<TaskId> {
        let mut newly_ready = Vec::new();

        if let Some(dependents) = self.adjacency.get(completed_task_id) {
            for dependent_id in dependents {
                if let Some(degree) = self.in_degree.get_mut(dependent_id) {
                    // 检查OR依赖的特殊处理
                    let task = states.get(dependent_id);
                    if let Some(WorkflowTask { dependencies: TaskDependencies::Any(dep_ids), .. }) = task {
                        // OR依赖：任一完成即可
                        if states.get(completed_task_id).map(|s| *s == TaskState::Completed).unwrap_or(false) {
                            *degree = 0;
                        }
                    } else {
                        *degree -= 1;
                    }

                    if *degree == 0 && matches!(states.get(dependent_id), Some(TaskState::Waiting)) {
                        newly_ready.push(dependent_id.clone());
                    }
                }
            }
        }

        newly_ready
    }
}
```

### WorkflowEngine

```rust
/// 工作流引擎
pub struct WorkflowEngine {
    /// 任务队列（复用现有）
    task_queue: Arc<TaskQueue>,
    /// 活跃的工作流
    workflows: RwLock<HashMap<WorkflowId, Workflow>>,
    /// 执行器
    executor: Arc<dyn WorkflowTaskExecutor>,
}

/// 工作流任务执行器trait
#[async_trait]
pub trait WorkflowTaskExecutor: Send + Sync {
    /// 执行单个任务
    async fn execute(&self, task: &BackgroundTask) -> Result<String, String>;
}

impl WorkflowEngine {
    /// 提交工作流
    pub async fn submit_workflow(&self, workflow: Workflow) -> Result<WorkflowId, WorkflowError> {
        let workflow_id = workflow.id.clone();
        
        // 验证工作流（无循环依赖等）
        self.validate_workflow(&workflow)?;
        
        // 存储工作流
        self.workflows.write().await.insert(workflow_id.clone(), workflow);
        
        // 启动工作流执行
        self.start_workflow(&workflow_id).await?;
        
        Ok(workflow_id)
    }

    /// 启动工作流执行
    async fn start_workflow(&self, workflow_id: &WorkflowId) -> Result<(), WorkflowError> {
        let mut workflow = self.workflows.write().await;
        let workflow = workflow.get_mut(workflow_id)
            .ok_or(WorkflowError::WorkflowNotFound)?;
        
        workflow.status = WorkflowStatus::Running;
        workflow.started_at = Some(chrono::Utc::now().timestamp_millis());
        
        // 获取初始可执行的任务
        let ready_tasks = workflow.graph.get_ready_tasks(
            &workflow.tasks.iter().map(|(k, v)| (k.clone(), v.state)).collect()
        );
        
        // 提交就绪任务
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
        
        match &task.definition {
            TaskDefinition::Simple(bg_task) => {
                // 更新状态为Pending
                task.state = TaskState::Pending;
                
                // 提交到现有任务队列
                let bg_task = bg_task.clone();
                let workflow_id = workflow_id.clone();
                let task_id = task_id.clone();
                let queue = Arc::clone(&self.task_queue);
                let executor = Arc::clone(&self.executor);
                
                tokio::spawn(async move {
                    // 创建包装任务，完成后回调
                    let wrapper_task = BackgroundTask::new(
                        bg_task.user_id.clone(),
                        bg_task.instruction.clone(),
                    );
                    
                    let submitted_id = queue.submit(wrapper_task).await;
                    
                    // 执行任务并获取结果
                    match executor.execute(&bg_task).await {
                        Ok(result) => {
                            queue.set_result(&submitted_id, result.clone()).await;
                            // 通知工作流引擎任务完成
                            // 这里使用消息通道或回调机制
                        }
                        Err(error) => {
                            queue.set_error(&submitted_id, error.clone()).await;
                        }
                    }
                });
            }
            TaskDefinition::SubWorkflow(sub_workflow) => {
                // 递归执行子工作流
                let sub_workflow = sub_workflow.clone();
                tokio::spawn(async move {
                    // 递归处理子工作流
                });
            }
            TaskDefinition::Parallel(tasks) => {
                // 并行执行所有任务
                let tasks = tasks.clone();
                tokio::spawn(async move {
                    // 使用futures::future::join_all并行执行
                });
            }
        }
        
        Ok(())
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
        
        // 更新任务状态
        match result {
            Ok(_) => {
                task.state = TaskState::Completed;
            }
            Err(_) => {
                task.state = TaskState::Failed;
                
                // 检查是否有备用路径
                if let Some(fallback_id) = &task.fallback {
                    // 触发备用任务
                    self.submit_task(workflow_id, fallback_id).await?;
                    return Ok(());
                }
            }
        }
        
        // 更新依赖图，获取新就绪的任务
        let states: HashMap<_, _> = workflow.tasks.iter()
            .map(|(k, v)| (k.clone(), v.state))
            .collect();
        let ready_tasks = workflow.graph.mark_completed(task_id, &states);
        
        // 提交新就绪的任务
        for ready_task_id in ready_tasks {
            if let Some(ready_task) = workflow.tasks.get(&ready_task_id) {
                // 检查条件依赖
                if self.check_conditions(&ready_task.dependencies, workflow) {
                    drop(workflows); // 释放锁
                    self.submit_task(workflow_id, &ready_task_id).await?;
                    workflows = self.workflows.write().await;
                } else {
                    // 条件不满足，标记为跳过
                    if let Some(task) = workflow.tasks.get_mut(&ready_task_id) {
                        task.state = TaskState::Skipped;
                    }
                }
            }
        }
        
        // 检查工作流是否完成
        self.check_workflow_completion(workflow);
        
        Ok(())
    }

    /// 检查条件依赖
    fn check_conditions(
        &self,
        dependencies: &TaskDependencies,
        workflow: &Workflow,
    ) -> bool {
        match dependencies {
            TaskDependencies::Condition { task_id, predicate } => {
                if let Some(task) = workflow.tasks.get(task_id) {
                    match predicate {
                        ConditionPredicate::Success => {
                            matches!(task.state, TaskState::Completed)
                        }
                        ConditionPredicate::ResultContains(text) => {
                            // 需要存储任务结果，这里简化处理
                            true
                        }
                        ConditionPredicate::Custom(_) => {
                            // 运行时解析执行
                            true
                        }
                    }
                } else {
                    false
                }
            }
            _ => true,
        }
    }

    /// 检查工作流是否完成
    fn check_workflow_completion(&self, workflow: &mut Workflow) {
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

    /// 验证工作流（检查循环依赖等）
    fn validate_workflow(&self, workflow: &Workflow) -> Result<(), WorkflowError> {
        // TODO: 实现拓扑排序检查循环依赖
        Ok(())
    }
}

/// 工作流错误类型
#[derive(Error, Debug)]
pub enum WorkflowError {
    #[error("Workflow not found: {0}")]
    WorkflowNotFound,
    #[error("Task not found: {0}")]
    TaskNotFound,
    #[error("Cyclic dependency detected")]
    CyclicDependency,
    #[error("Invalid workflow configuration: {0}")]
    InvalidConfiguration(String),
}
```

## API Design

### DSL 示例

```rust
// 构建一个AI研究工作流
let workflow = WorkflowBuilder::new("AI Research")
    .description("多源搜索、验证、整合、生成报告")
    .task("search_sources", TaskDefinition::Simple(search_task))
        .with_retry(RetryPolicy {
            max_retries: 2,
            retry_interval_ms: 1000,
            exponential_backoff: true,
        })
    .task("validate_info", TaskDefinition::Simple(validate_task))
        .depends_on(TaskDependencies::Sequential("search_sources".into()))
    .task("alternative_search", TaskDefinition::Simple(alt_search_task))
    .task("integrate", TaskDefinition::Simple(integrate_task))
        .depends_on(TaskDependencies::Condition {
            task_id: "validate_info".into(),
            predicate: ConditionPredicate::Success,
        })
        .fallback("alternative_search")
    .task("generate_report", TaskDefinition::Simple(report_task))
        .depends_on(TaskDependencies::Sequential("integrate".into()))
    .build();
```

### JSON Schema

```json
{
  "id": "workflow_123",
  "name": "AI Research",
  "description": "多源搜索、验证、整合、生成报告",
  "tasks": {
    "search_sources": {
      "definition": {
        "type": "Simple",
        "task": {
          "instruction": "搜索相关技术资料",
          "priority": "High"
        }
      },
      "dependencies": "None",
      "retry_policy": {
        "max_retries": 2,
        "retry_interval_ms": 1000
      }
    },
    "validate_info": {
      "definition": {
        "type": "Simple",
        "task": {
          "instruction": "验证信息准确性"
        }
      },
      "dependencies": {
        "type": "Sequential",
        "task_id": "search_sources"
      }
    },
    "integrate": {
      "definition": {
        "type": "Simple",
        "task": {
          "instruction": "整合验证后的信息"
        }
      },
      "dependencies": {
        "type": "Condition",
        "task_id": "validate_info",
        "predicate": "Success"
      },
      "fallback": "alternative_search"
    }
  }
}
```

## Integration with Existing System

### 与 TaskQueue 集成

```rust
impl WorkflowEngine {
    /// 将工作流任务转换为BackgroundTask并提交
    async fn enqueue_task(
        &self,
        workflow_id: &WorkflowId,
        task_id: &TaskId,
        bg_task: &BackgroundTask,
    ) -> Result<(), WorkflowError> {
        // 包装任务以支持回调
        let wrapped_task = bg_task.clone();
        
        // 提交到现有队列
        let submitted_id = self.task_queue.submit(wrapped_task).await;
        
        // 启动监听任务完成的task
        let workflow_id = workflow_id.clone();
        let task_id = task_id.clone();
        let queue = Arc::clone(&self.task_queue);
        let engine_weak = Arc::downgrade(&self);
        
        tokio::spawn(async move {
            // 轮询或使用通知机制等待任务完成
            loop {
                if let Some(task) = queue.get(&submitted_id).await {
                    if task.is_finished() {
                        let result = if task.status == TaskStatus::Completed {
                            Ok(task.result.unwrap_or_default())
                        } else {
                            Err(task.error.unwrap_or_default())
                        };
                        
                        if let Some(engine) = engine_weak.upgrade() {
                            let _ = engine.on_task_completed(&workflow_id, &task_id, result).await;
                        }
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        
        Ok(())
    }
}
```

### 与 Gateway 集成

```rust
// 在Hub中添加Workflow支持
impl Hub {
    pub async fn submit_workflow(&self, workflow: Workflow) -> Result<WorkflowId, GatewayError> {
        self.workflow_engine.submit_workflow(workflow).await
            .map_err(|e| GatewayError::WorkflowError(e.to_string()))
    }
    
    pub async fn get_workflow_status(&self, workflow_id: &WorkflowId) -> Option<WorkflowStatus> {
        self.workflow_engine.get_status(workflow_id).await
    }
}
```

## Error Handling

1. **依赖失败**：如果依赖任务失败，且没有定义fallback，则当前任务标记为Failed
2. **循环依赖**：在提交时进行拓扑排序检测，发现循环则返回错误
3. **超时处理**：每个任务可以设置超时时间，超时后自动失败
4. **工作流取消**：支持取消整个工作流，级联取消所有正在执行的任务

## Performance Considerations

1. **并发控制**：使用现有的TaskScheduler控制并发数
2. **内存优化**：工作流状态持久化到SQLite，减少内存占用
3. **事件驱动**：使用tokio channels进行任务完成通知，避免轮询

## Future Enhancements

1. **可视化编辑器**：Web界面支持拖拽式工作流编辑
2. **模板库**：预设常见AI研究流程的模板
3. **分布式执行**：支持多节点任务分发
4. **动态修改**：运行时添加/移除任务

## Appendix: Migration from Current System

当前系统使用`BackgroundTask`直接提交到`TaskQueue`。迁移步骤：

1. 保留现有的`TaskQueue`和`BackgroundTask`不变
2. 新增`WorkflowEngine`作为上层抽象
3. 逐步将直接提交的任务改为通过Workflow编排
4. API层新增workflow相关端点

## References

- 现有代码：`src/gateway/task_queue.rs`
- 现有代码：`src/core/task_scheduler.rs`
- DAG算法：https://en.wikipedia.org/wiki/Topological_sorting
