//! 对话消息类型定义。
//!
//! 采用 OpenAI 风格的消息结构，按角色（system/user/assistant/tool）区分，
//! 支持文本内容、工具调用和推理过程（thinking）。

use serde::{Deserialize, Serialize};

/// 对话消息枚举，按角色通过 serde tag 进行序列化区分
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "role")]
pub enum Message {
    #[serde(rename = "system")]
    System { content: String },

    #[serde(rename = "user")]
    User { content: String },

    #[serde(rename = "assistant")]
    Assistant {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ToolCall>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning: Option<String>,
    },

    #[serde(rename = "tool")]
    Tool {
        tool_call_id: String,
        content: String,
    },
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::User {
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::Assistant {
            content: Some(content.into()),
            tool_calls: Vec::new(),
            reasoning: None,
        }
    }

    /// 创建带工具调用的助手消息
    pub fn assistant_with_tool_calls(
        content: Option<String>,
        tool_calls: Vec<ToolCall>,
    ) -> Self {
        Self::Assistant {
            content,
            tool_calls,
            reasoning: None,
        }
    }

    /// 创建工具执行结果消息
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self::Tool {
            tool_call_id: tool_call_id.into(),
            content: content.into(),
        }
    }
}

/// 工具调用描述：包含调用 ID、工具名称和 JSON 参数
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}
