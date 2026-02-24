//! 工作流类型定义
//!
//! 定义工作流、任务、依赖关系等核心数据类型

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(feature = "gateway")]
use crate::gateway::BackgroundTask;

pub type WorkflowId = String;
pub type TaskId = String;

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
    /// 当前状态
    pub status: WorkflowStatus,
    /// 创建时间
    pub created_at: i64,
    /// 开始执行时间
    pub started_at: Option<i64>,
    /// 完成时间
    pub completed_at: Option<i64>,
}

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
    /// 执行状态
    pub state: TaskState,
}

/// 任务定义
#[cfg(feature = "gateway")]
pub enum TaskDefinition {
    /// 简单任务：复用现有的BackgroundTask
    Simple(BackgroundTask),
    /// 子工作流：嵌套另一个工作流
    SubWorkflow(Box<Workflow>),
    /// 并行任务组：Map模式
    Parallel(Vec<BackgroundTask>),
}

#[cfg(not(feature = "gateway"))]
pub enum TaskDefinition {
    /// 子工作流：嵌套另一个工作流
    SubWorkflow(Box<Workflow>),
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
}

/// 工作流错误类型
#[derive(Error, Debug)]
pub enum WorkflowError {
    #[error("Workflow not found")]
    WorkflowNotFound,
    #[error("Task not found")]
    TaskNotFound,
    #[error("Cyclic dependency detected")]
    CyclicDependency,
    #[error("Invalid workflow configuration: {0}")]
    InvalidConfiguration(String),
}
