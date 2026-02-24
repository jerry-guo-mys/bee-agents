# Workflow System

Bee 的工作流系统支持复杂的任务依赖编排，包括顺序、并行、条件和嵌套工作流。

## Quick Start

```rust
use bee::workflow::*;
use bee::gateway::{BackgroundTask, TaskQueue};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // 创建任务队列和执行器
    let (queue, _, _) = TaskQueue::new();
    let executor = Arc::new(MyExecutor);
    
    // 创建工作流引擎
    let engine = WorkflowEngine::new(
        Arc::new(queue),
        executor,
    );
    
    // 构建工作流
    let workflow = WorkflowBuilder::new("My Workflow")
        .user_id("user1".to_string())
        .task("step1", BackgroundTask::new("user1".to_string(), "First step".to_string()))
        .task("step2", BackgroundTask::new("user1".to_string(), "Second step".to_string()))
        .sequential("step1", "step2")
        .build()
        .unwrap();
    
    // 提交工作流
    let workflow_id = engine.submit_workflow(workflow).await.unwrap();
    println!("Workflow submitted: {}", workflow_id);
}
```

## Dependency Types

### Sequential (顺序依赖)
一个任务完成后执行下一个：

```rust
let workflow = WorkflowBuilder::new("Sequential")
    .user_id("user1".to_string())
    .task("a", task_a)
    .task("b", task_b)
    .sequential("a", "b")  // b 在 a 完成后执行
    .build()?;
```

### All (AND 依赖)
所有前置任务完成后执行：

```rust
let workflow = WorkflowBuilder::new("Parallel")
    .user_id("user1".to_string())
    .task("a", task_a)
    .task("b", task_b)
    .task("c", task_c)
    .depends_on_all("c", vec!["a".to_string(), "b".to_string()])
    .build()?;
```

### Any (OR 依赖)
任一前置任务完成后执行：

```rust
let workflow = WorkflowBuilder::new("OR")
    .user_id("user1".to_string())
    .task("a", task_a)
    .task("b", task_b)
    .task("c", task_c)
    .depends_on_any("c", vec!["a".to_string(), "b".to_string()])
    .build()?;
```

### Condition (条件依赖)
前置任务满足特定条件：

```rust
// 需要在构建后手动设置条件
let mut workflow = WorkflowBuilder::new("Condition")
    .user_id("user1".to_string())
    .task("check", check_task)
    .task("process", process_task)
    .build()?;

// 设置条件依赖
if let Some(task) = workflow.tasks.get_mut("process") {
    task.dependencies = TaskDependencies::Condition {
        task_id: "check".to_string(),
        predicate: ConditionPredicate::Success,
    };
}
```

## Features

- **DAG-based execution**: 基于有向无环图的任务调度
- **Fallback paths on failure**: 任务失败时自动切换到备用路径
- **Nested sub-workflows**: 支持子工作流嵌套（计划中）
- **Parallel task groups**: 并行任务组
- **Integration with existing TaskQueue**: 与现有任务队列无缝集成

## API Reference

### WorkflowBuilder

| Method | Description |
|--------|-------------|
| `new(name)` | 创建构建器 |
| `description(desc)` | 设置描述 |
| `user_id(id)` | 设置用户 ID（必需） |
| `session_id(id)` | 设置会话 ID |
| `task(id, task)` | 添加任务 |
| `sequential(from, to)` | 设置顺序依赖 |
| `depends_on_all(task, deps)` | 设置 AND 依赖 |
| `depends_on_any(task, deps)` | 设置 OR 依赖 |
| `with_fallback(task, fallback)` | 设置失败备用 |
| `build()` | 构建工作流 |

### WorkflowEngine

| Method | Description |
|--------|-------------|
| `new(queue, executor)` | 创建引擎 |
| `submit_workflow(workflow)` | 提交工作流 |
| `get_status(id)` | 获取状态 |
| `on_task_completed(id, task_id, result)` | 任务完成回调 |

## Examples

参见 `examples/workflow_example.rs` 完整示例。

## Testing

运行测试：

```bash
cargo test workflow --features gateway
cargo test --test workflow_integration_test --features gateway
```

## Future Enhancements

1. 可视化编辑器 - Web 界面支持拖拽式工作流编辑
2. 模板库 - 预设常见 AI 研究流程的模板
3. 分布式执行 - 支持多节点任务分发
4. 动态修改 - 运行时添加/移除任务
