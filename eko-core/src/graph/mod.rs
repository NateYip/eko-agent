//! 状态图引擎模块。
//!
//! 实现了类似 LangGraph 的状态图执行引擎。通过 `StateGraph` 构建图拓扑，
//! `compile()` 后得到不可变的 `CompiledGraph` 运行时，支持 invoke/resume 执行。

pub mod builder;
pub mod command;
pub mod compiled;
pub mod edge;
pub mod node;
pub(crate) mod pregel;
pub mod send;
pub mod state_schema;
pub mod subgraph;
pub mod task;
pub mod types;

pub use builder::StateGraph;
pub use command::{CommandGraph, GotoTarget, NodeOutput};
pub use compiled::CompiledGraph;
pub use edge::{Edge, RouteDecision, END, START};
pub use node::{FnNode, GraphNode, NodeContext};
pub use send::Send;
pub use state_schema::State;
pub use types::{CompileConfig, Durability, GraphConfig, GraphResult, InterruptReason, RetryPolicy, StateSnapshot};

pub use types::Command;
