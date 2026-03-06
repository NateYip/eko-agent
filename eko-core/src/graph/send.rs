//! Send 数据结构：用于条件边路由时的并行扇出。
//!
//! 路由函数可以返回多个 `Send`，每个 `Send` 会触发目标节点的一个并行 PUSH 任务，
//! 以 `args` 作为该任务的输入（而非全局 state）。

use serde_json::Value;

/// 并行扇出指令：将 `args` 作为输入发送给指定节点的 PUSH 任务
#[derive(Debug, Clone)]
pub struct Send {
    /// 目标节点名
    pub node: String,
    /// 该任务的自定义输入参数
    pub args: Value,
}

impl Send {
    pub fn new(node: impl Into<String>, args: Value) -> Self {
        Self {
            node: node.into(),
            args,
        }
    }
}
