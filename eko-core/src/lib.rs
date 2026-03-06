//! eko-core: AI Agent 的核心运行时库。
//!
//! 本库实现了基于状态图（StateGraph）的图执行引擎，
//! 提供通道系统、检查点抽象、流式事件等核心契约。
//! 具体的 LLM 提供商、预置图模式和持久化后端由上层库提供。

pub mod channels;
pub mod checkpoint;
pub mod core;
pub mod graph;

pub use crate::core::{
    ActionType, AgentError, InteractionOption, InteractionRequest, InteractionResponse,
    InterruptData, LlmClient, LlmStreamItem, Message, MessageType, ResponseAction,
    StreamEvent, StreamMode, ToolCall, ToolDefinition,
};

pub use crate::checkpoint::{BaseCheckpointSaver, MemorySaver};

pub use crate::graph::{
    Command, CommandGraph, CompileConfig, CompiledGraph, FnNode, GraphConfig, GraphNode,
    GraphResult, InterruptReason, NodeOutput, RetryPolicy, State, StateGraph, StateSnapshot,
    END, START,
};
pub use crate::graph::types::Durability;

pub use crate::channels::{
    overwrite, AnyChannel, BinaryOperatorAggregate, ChannelValues, DynamicBarrierValue,
    EphemeralValue, LastValue, NamedBarrierValue, Reducer, Topic, OVERWRITE_KEY,
};

/// Re-export the derive macro so users can write `#[derive(State)]`.
pub use eko_core_derive::State;
