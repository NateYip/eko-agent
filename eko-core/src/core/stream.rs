//! 流式事件定义。
//!
//! 定义了 Agent 执行过程中向客户端推送的所有事件类型，包括文本流、
//! 工具调用、任务列表、文件操作等。前端通过订阅这些事件实现实时展示。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::interaction::InteractionRequest;
use super::message::ToolCall as ToolCallData;

/// Agent 推送给客户端的流式事件枚举
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    /// 对话流开始
    #[serde(rename = "stream.start")]
    Start { thread_id: String, timestamp: i64 },

    /// 文本或思考过程的增量消息
    #[serde(rename = "stream.message")]
    Message {
        #[serde(rename = "message_type")]
        message_type: MessageType,
        content: String,
        timestamp: i64,
    },

    /// 完整的工具调用事件
    #[serde(rename = "stream.tool_call")]
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
        timestamp: i64,
    },

    /// 工具调用参数的增量片段（用于流式构建工具调用）
    #[serde(rename = "stream.tool_call_chunk")]
    ToolCallChunk {
        id: String,
        name: Option<String>,
        args: String,
        index: usize,
        timestamp: i64,
    },

    /// 工具执行结果
    #[serde(rename = "stream.tool_result")]
    ToolResult {
        tool_call_id: String,
        name: String,
        result: serde_json::Value,
        success: bool,
        timestamp: i64,
    },

    /// 图在 tools 节点中断后发出，应用层需执行工具调用后调用 resume 恢复
    #[serde(rename = "stream.tool_call_requested")]
    ToolCallRequested {
        thread_id: String,
        tool_calls: Vec<ToolCallData>,
    },

    /// 图中断事件（非工具确认的其他中断）
    #[serde(rename = "stream.interrupt")]
    Interrupt {
        node: String,
        reason: String,
    },

    /// 任务列表更新事件
    #[serde(rename = "stream.todos")]
    Todos {
        todos: Vec<TodoItem>,
        timestamp: i64,
    },

    /// 文件系统操作事件
    #[serde(rename = "stream.filesystem")]
    Filesystem {
        operation: FilesystemOp,
        timestamp: i64,
    },

    /// 对话流结束
    #[serde(rename = "stream.done")]
    Done { thread_id: String },

    /// 错误事件
    #[serde(rename = "stream.error")]
    Error { error: String, timestamp: i64 },

    /// 子图穿透事件：子图内部事件携带命名空间冒泡到父图
    #[serde(rename = "stream.subgraph")]
    SubgraphEvent {
        namespace: String,
        event: Box<StreamEvent>,
    },

    // ---- 图引擎层事件（Pregel 新增） ----
    /// superstep 结束后的完整状态快照
    #[serde(rename = "stream.state_values")]
    StateValues {
        values: std::collections::HashMap<String, serde_json::Value>,
        step: i64,
    },

    /// 单节点产出的增量状态更新
    #[serde(rename = "stream.state_update")]
    StateUpdate {
        node: String,
        updates: std::collections::HashMap<String, serde_json::Value>,
    },

    /// 检查点已持久化
    #[serde(rename = "stream.checkpoint_saved")]
    CheckpointSaved {
        checkpoint_id: String,
        step: i64,
    },

    /// 节点任务开始执行
    #[serde(rename = "stream.task_started")]
    TaskStarted { node: String },

    /// 节点任务执行完成
    #[serde(rename = "stream.task_completed")]
    TaskCompleted { node: String },

    /// 节点通过 emit() 发出的自定义事件
    #[serde(rename = "stream.custom")]
    Custom {
        name: String,
        data: serde_json::Value,
    },

    /// 节点请求外部交互（Outer-in-the-Loop）
    #[serde(rename = "stream.interaction_required")]
    InteractionRequired {
        request: InteractionRequest,
    },
}

/// 消息内容类型：普通文本或思考过程
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Text,
    Thinking,
}

/// 工具执行请求，包含是否自动执行的标记
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRequest {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args: serde_json::Value,
    pub auto_execute: bool,
}

/// 任务项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
}

/// 任务状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
    Cancelled,
}

/// 文件系统操作描述
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemOp {
    #[serde(rename = "type")]
    pub op_type: FilesystemOpType,
    pub path: String,
    pub content: Option<String>,
}

/// 文件操作类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemOpType {
    Create,
    Write,
    Delete,
}

/// 流式输出模式：控制客户端接收哪类事件
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamMode {
    /// 每步结束后的完整状态值
    Values,
    /// 单节点增量更新
    Updates,
    /// LLM 文本流消息
    Messages,
    /// 工具调用事件
    Tools,
    /// 节点 emit() 自定义事件
    Custom,
    /// 调试事件（TaskStarted/TaskCompleted 等）
    Debug,
    /// 检查点保存事件
    Checkpoints,
}

impl StreamMode {
    pub fn default_set() -> HashSet<Self> {
        HashSet::from([Self::Values, Self::Updates, Self::Messages, Self::Tools])
    }

    pub fn all() -> HashSet<Self> {
        HashSet::from([
            Self::Values,
            Self::Updates,
            Self::Messages,
            Self::Tools,
            Self::Custom,
            Self::Debug,
            Self::Checkpoints,
        ])
    }
}

impl StreamEvent {
    /// 返回此事件对应的 StreamMode（用于过滤）
    pub fn mode(&self) -> Option<StreamMode> {
        match self {
            Self::StateValues { .. } => Some(StreamMode::Values),
            Self::StateUpdate { .. } => Some(StreamMode::Updates),
            Self::Message { .. } => Some(StreamMode::Messages),
            Self::ToolCall { .. }
            | Self::ToolCallChunk { .. }
            | Self::ToolResult { .. }
            | Self::ToolCallRequested { .. } => Some(StreamMode::Tools),
            Self::Custom { .. } => Some(StreamMode::Custom),
            Self::TaskStarted { .. } | Self::TaskCompleted { .. } => Some(StreamMode::Debug),
            Self::CheckpointSaved { .. } => Some(StreamMode::Checkpoints),
            _ => None,
        }
    }
}

impl StreamEvent {
    pub fn now() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    pub fn start(thread_id: impl Into<String>) -> Self {
        Self::Start {
            thread_id: thread_id.into(),
            timestamp: Self::now(),
        }
    }

    pub fn message(message_type: MessageType, content: impl Into<String>) -> Self {
        Self::Message {
            message_type,
            content: content.into(),
            timestamp: Self::now(),
        }
    }

    pub fn text(content: impl Into<String>) -> Self {
        Self::message(MessageType::Text, content)
    }

    pub fn tool_call(
        id: impl Into<String>,
        name: impl Into<String>,
        args: serde_json::Value,
    ) -> Self {
        Self::ToolCall {
            id: id.into(),
            name: name.into(),
            args,
            timestamp: Self::now(),
        }
    }

    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        result: serde_json::Value,
        success: bool,
    ) -> Self {
        Self::ToolResult {
            tool_call_id: tool_call_id.into(),
            name: name.into(),
            result,
            success,
            timestamp: Self::now(),
        }
    }

    pub fn done(thread_id: impl Into<String>) -> Self {
        Self::Done {
            thread_id: thread_id.into(),
        }
    }

    pub fn error(error: impl Into<String>) -> Self {
        Self::Error {
            error: error.into(),
            timestamp: Self::now(),
        }
    }
}
