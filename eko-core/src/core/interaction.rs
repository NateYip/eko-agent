//! 外部交互协议（Outer-in-the-Loop）。
//!
//! 提供通用的交互请求/响应类型。core 不关心响应者是人、Agent 还是系统——
//! 只提供透明的 `operator_id` 标记槽位，由上层自行赋予语义
//! （如 `"user:123"`、`"agent:swarm-1"`、`"system"` 等）。
//!
//! 节点通过 `NodeContext::request_interaction()` 发出请求，
//! 图引擎通过 `StreamEvent::InteractionRequired` 通知外部。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 交互请求：节点向外部发起的操作请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionRequest {
    pub id: String,
    pub node: String,
    pub action_type: ActionType,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<InteractionOption>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// 可选项（用于 Select 类型交互）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionOption {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 请求的操作类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Confirm,
    Select,
    Input,
    Review,
    Approve,
    Custom(String),
}

/// 交互响应：外部对请求的回复
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractionResponse {
    pub request_id: String,
    /// 透明的操作者标识，core 不解析其格式。
    /// 上层自行决定语义，例如 `"user:123"`、`"agent:swarm-1"`、`"system"` 等。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operator_id: Option<String>,
    pub action: ResponseAction,
}

/// 响应动作
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ResponseAction {
    Accept,
    Reject {
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Select {
        selected: Vec<String>,
    },
    Input {
        value: Value,
    },
}
