//! LLM 客户端抽象接口。
//!
//! 定义了与大语言模型交互的 trait `LlmClient` 和流式响应数据项 `LlmStreamItem`。
//! 具体的 LLM 提供商（如 OpenAI）在 providers 模块中实现此 trait。

use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;

use super::message::{Message, ToolCall};
use super::tool::ToolDefinition;

/// LLM 流式响应中的单个数据项
#[derive(Debug, Clone)]
pub enum LlmStreamItem {
    /// 文本增量片段
    TextDelta(String),
    /// 完整的工具调用请求
    ToolCall(ToolCall),
    /// 推理过程的增量片段（思维链）
    ReasoningDelta(String),
    /// 流结束标记
    Done,
}

/// LLM 客户端的抽象接口，实现方需提供流式对话能力
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// 发起流式对话请求，返回异步流逐步产出 LlmStreamItem
    async fn stream_chat(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<LlmStreamItem>> + Send>>>;
}
