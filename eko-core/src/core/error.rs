//! Agent 错误类型定义。
//!
//! 按错误来源分类：LLM 调用、配置、工具执行、会话管理、图执行、节点中断等。

use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

/// 节点内中断时携带的数据
#[derive(Debug, Clone)]
pub struct InterruptData {
    /// 中断在节点内的序号（同一节点可多次中断）
    pub index: usize,
    /// 中断时传出的值（展示给用户）
    pub value: Value,
    /// 发起中断的节点名
    pub node_name: String,
}

/// Agent 统一错误枚举，涵盖运行时可能出现的各类错误
#[derive(Debug, Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Tool error: {0}")]
    Tool(String),
    #[error("Session error: {0}")]
    Session(String),
    #[error("Graph error: {0}")]
    GraphError(String),
    #[error("Node interrupted at index {}", .0.index)]
    Interrupt(InterruptData),
    #[error("Recursion limit exceeded: reached step {step} (limit {limit})")]
    RecursionLimit { limit: usize, step: usize },
    #[error("Invalid update on channel '{channel}': {reason}")]
    InvalidUpdate { channel: String, reason: String },
    #[error("Unreachable node: '{0}' has no outgoing edge")]
    UnreachableNode(String),
    #[error("Node '{node}' timed out after {duration:?}")]
    NodeTimeout { node: String, duration: Duration },
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

impl AgentError {
    /// 判断此错误是否可被 RetryPolicy 自动重试。
    /// LLM 和未分类错误视为可重试，中断和图结构错误不可重试。
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Llm(_) | Self::Other(_))
    }
}
