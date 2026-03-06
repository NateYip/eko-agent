//! 核心类型模块。
//!
//! 汇聚了 Agent 运行时使用的基础类型：错误定义、LLM 客户端接口、
//! 消息结构、流式事件和工具定义。

pub mod error;
pub mod interaction;
pub mod llm;
pub mod message;
pub mod stream;
pub mod tool;

pub use error::{AgentError, InterruptData};
pub use interaction::{
    ActionType, InteractionOption, InteractionRequest, InteractionResponse, ResponseAction,
};
pub use llm::{LlmClient, LlmStreamItem};
pub use message::{Message, ToolCall};
pub use stream::{MessageType, StreamEvent, StreamMode};
pub use tool::ToolDefinition;
