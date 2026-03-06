//! Task 抽象：区分 PULL（常规边触发）和 PUSH（Send 触发）两种执行任务。

use serde_json::Value;
use uuid::Uuid;

/// 任务类型
#[derive(Debug, Clone)]
pub enum TaskType {
    /// 常规节点执行：输入来自全局 state channels
    Pull,
    /// Send 触发的并行任务：输入来自 Send.args
    Push(Value),
}

/// 图执行引擎中的一个执行单元
#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub node_name: String,
    pub task_type: TaskType,
}

impl Task {
    pub fn pull(node_name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            node_name: node_name.into(),
            task_type: TaskType::Pull,
        }
    }

    pub fn push(node_name: impl Into<String>, args: Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            node_name: node_name.into(),
            task_type: TaskType::Push(args),
        }
    }
}
