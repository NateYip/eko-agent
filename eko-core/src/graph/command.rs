//! 节点输出与命令定义。
//!
//! `NodeOutput` 是图节点 `process` 方法的返回类型，支持两种模式：
//! - `Update`：普通通道更新
//! - `Command`：带路由指令的更新（节点自主决定下一跳）

use crate::channels::ChannelValues;

use super::send::Send as GraphSend;

/// Command.goto 的目标类型：单节点跳转或并行扇出
#[derive(Debug, Clone)]
pub enum GotoTarget {
    /// 跳转到单个节点
    Node(String),
    /// 并行扇出：每个 Send 触发一个 PUSH 任务
    Sends(Vec<GraphSend>),
}

/// 图节点的输出类型
pub enum NodeOutput {
    /// 普通通道更新（现有行为）
    Update(ChannelValues),
    /// 带路由指令的命令
    Command {
        update: Option<ChannelValues>,
        /// 节点自主决定下一跳，支持单节点或并行扇出
        goto: Option<GotoTarget>,
        /// 指定命令作用的图层级（当前图或父图）
        graph: Option<CommandGraph>,
    },
}

/// 命令作用的图层级
#[derive(Debug, Clone, PartialEq)]
pub enum CommandGraph {
    /// 命令作用于当前图
    Current,
    /// 命令冒泡到父图（子图场景）
    Parent,
}

impl NodeOutput {
    /// 便捷构造：纯通道更新
    pub fn update(values: ChannelValues) -> Self {
        Self::Update(values)
    }

    /// 便捷构造：带单节点路由的命令
    pub fn command(update: Option<ChannelValues>, goto: impl Into<String>) -> Self {
        Self::Command {
            update,
            goto: Some(GotoTarget::Node(goto.into())),
            graph: None,
        }
    }

    /// 便捷构造：带并行扇出路由的命令
    pub fn command_with_sends(update: Option<ChannelValues>, sends: Vec<GraphSend>) -> Self {
        Self::Command {
            update,
            goto: Some(GotoTarget::Sends(sends)),
            graph: None,
        }
    }

    /// 提取通道更新（无论哪种变体）
    pub fn take_updates(&self) -> Option<&ChannelValues> {
        match self {
            Self::Update(v) => Some(v),
            Self::Command { update, .. } => update.as_ref(),
        }
    }

    /// 提取 goto 目标（完整的 GotoTarget 引用）
    pub fn goto_target(&self) -> Option<&GotoTarget> {
        match self {
            Self::Update(_) => None,
            Self::Command { goto, .. } => goto.as_ref(),
        }
    }

    /// 提取 goto 目标节点名（仅 GotoTarget::Node 时返回）
    pub fn goto(&self) -> Option<&str> {
        match self {
            Self::Update(_) => None,
            Self::Command { goto, .. } => match goto {
                Some(GotoTarget::Node(name)) => Some(name.as_str()),
                _ => None,
            },
        }
    }

    /// 提取命令的图层级
    pub fn graph(&self) -> Option<&CommandGraph> {
        match self {
            Self::Update(_) => None,
            Self::Command { graph, .. } => graph.as_ref(),
        }
    }
}

impl From<ChannelValues> for NodeOutput {
    fn from(values: ChannelValues) -> Self {
        Self::Update(values)
    }
}
