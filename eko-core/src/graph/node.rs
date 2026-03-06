//! 图节点定义。
//!
//! 定义了 `GraphNode` trait、执行上下文 `NodeContext` 和闭包节点 `FnNode`。
//! 每个节点接收全量通道值，返回 `NodeOutput`（通道更新或带路由的命令），
//! 图引擎再通过各通道的合并策略应用更新。

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use crate::channels::ChannelValues;
use crate::core::error::{AgentError, InterruptData};
use crate::core::interaction::InteractionRequest;
use crate::core::stream::StreamEvent;

use super::command::NodeOutput;
use super::types::RetryPolicy;

/// 节点执行上下文：提供事件流发送端、取消信号和中断/恢复机制
pub struct NodeContext {
    pub stream_tx: mpsc::UnboundedSender<StreamEvent>,
    pub cancel: watch::Receiver<bool>,
    interrupt_counter: AtomicI32,
    resume_values: Vec<Value>,
    node_name: String,
}

impl NodeContext {
    pub fn new(
        stream_tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: watch::Receiver<bool>,
    ) -> Self {
        Self {
            stream_tx,
            cancel,
            interrupt_counter: AtomicI32::new(0),
            resume_values: Vec::new(),
            node_name: String::new(),
        }
    }

    pub(crate) fn with_node_name(mut self, name: impl Into<String>) -> Self {
        self.node_name = name.into();
        self
    }

    pub(crate) fn with_resume_values(mut self, values: Vec<Value>) -> Self {
        self.resume_values = values;
        self
    }

    /// 检查是否已被用户取消
    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    /// 节点内中断。若有对应的 resume 值则返回 `Ok(value)`，否则暂停执行并返回 `Err`。
    /// 同一节点可多次调用，按调用顺序与 resume 值一一对应。
    pub fn interrupt(&self, value: Value) -> Result<Value, AgentError> {
        let idx = self.interrupt_counter.fetch_add(1, Ordering::SeqCst) as usize;
        if idx < self.resume_values.len() {
            Ok(self.resume_values[idx].clone())
        } else {
            Err(AgentError::Interrupt(InterruptData {
                index: idx,
                value,
                node_name: self.node_name.clone(),
            }))
        }
    }

    /// 发送自定义流式事件，用于节点向外部推送领域相关数据。
    /// 事件需要客户端启用 `StreamMode::Custom` 才会收到。
    pub fn emit(&self, name: impl Into<String>, data: serde_json::Value) {
        let _ = self.stream_tx.send(StreamEvent::Custom {
            name: name.into(),
            data,
        });
    }

    /// 请求外部交互（Outer-in-the-Loop）。发送 InteractionRequired 事件后，
    /// 调用 `interrupt()` 暂停执行等待响应。外部通过 resume 返回 InteractionResponse。
    pub fn request_interaction(
        &self,
        request: InteractionRequest,
    ) -> Result<Value, AgentError> {
        let _ = self
            .stream_tx
            .send(StreamEvent::InteractionRequired { request: request.clone() });
        self.interrupt(serde_json::to_value(&request).unwrap_or_default())
    }

    /// 创建一个保留 resume 状态的副本（供 FnNode 内部使用）
    pub(crate) fn clone_for_fn(&self) -> Self {
        Self {
            stream_tx: self.stream_tx.clone(),
            cancel: self.cancel.clone(),
            interrupt_counter: AtomicI32::new(0),
            resume_values: self.resume_values.clone(),
            node_name: self.node_name.clone(),
        }
    }
}

/// 图节点 trait：接收全量通道状态，返回 `NodeOutput`。
/// 图引擎会通过各通道的 reducer/覆盖逻辑将更新合并回全局状态。
#[async_trait]
pub trait GraphNode: Send + Sync {
    fn name(&self) -> &str;

    async fn process(
        &self,
        state: ChannelValues,
        ctx: &NodeContext,
    ) -> Result<NodeOutput, AgentError>;

    /// 节点级重试策略，返回 None 则使用图级默认策略
    fn retry_policy(&self) -> Option<&RetryPolicy> {
        None
    }

    /// 节点级执行超时，返回 None 则使用图级默认超时
    fn timeout(&self) -> Option<Duration> {
        None
    }
}

// ---------------------------------------------------------------------------
// FnNode: 闭包节点
// ---------------------------------------------------------------------------

type AsyncNodeFn = Box<
    dyn Fn(
            ChannelValues,
            NodeContext,
        ) -> Pin<Box<dyn Future<Output = Result<NodeOutput, AgentError>> + Send>>
        + Send
        + Sync,
>;

/// 闭包节点：将 async 闭包包装为 `GraphNode`，免去实现 trait 的样板代码。
pub struct FnNode {
    node_name: String,
    func: AsyncNodeFn,
}

impl FnNode {
    pub fn new<F, Fut>(name: impl Into<String>, func: F) -> Self
    where
        F: Fn(ChannelValues, NodeContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<NodeOutput, AgentError>> + Send + 'static,
    {
        Self {
            node_name: name.into(),
            func: Box::new(move |state, ctx| Box::pin(func(state, ctx))),
        }
    }
}

#[async_trait]
impl GraphNode for FnNode {
    fn name(&self) -> &str {
        &self.node_name
    }

    async fn process(
        &self,
        state: ChannelValues,
        ctx: &NodeContext,
    ) -> Result<NodeOutput, AgentError> {
        let owned_ctx = ctx.clone_for_fn();
        (self.func)(state, owned_ctx).await
    }
}
