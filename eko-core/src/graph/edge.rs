//! 图的边（Edge）定义。
//!
//! 支持三种边类型：静态跳转、条件路由和终止。
//! 条件路由通过闭包在运行时根据当前通道状态动态决定下一个节点。

use crate::channels::ChannelValues;

use super::send::Send as GraphSend;

/// 虚拟入口节点标识，用于 `add_edge(START, "node")` 语法
pub const START: &str = "__start__";

/// 终止节点的特殊标识
pub const END: &str = "__end__";

/// 条件路由的决策结果
#[derive(Debug, Clone)]
pub enum RouteDecision {
    /// 跳转到指定节点
    Next(String),
    /// 图执行结束
    End,
    /// 并行扇出：每个 Send 触发一个 PUSH 任务
    Send(Vec<GraphSend>),
}

/// 图中的边类型
pub enum Edge {
    /// 静态跳转：始终跳转到固定的目标节点
    Static(String),
    /// 条件路由：根据当前状态动态决定跳转目标
    Conditional(Box<dyn Fn(&ChannelValues) -> RouteDecision + Send + Sync>),
    /// 终止边：执行到此节点后图结束
    End,
}
