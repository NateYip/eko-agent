//! 子图节点：将 CompiledGraph 包装为 GraphNode。
//!
//! 子图执行时使用独立的 checkpoint namespace 实现状态隔离，
//! 子图内部的 StreamEvent 通过 SubgraphStreamWrapper 冒泡到父图。

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::channels::ChannelValues;
use crate::core::error::AgentError;
use crate::core::stream::StreamEvent;

use super::command::{CommandGraph, NodeOutput};
use super::compiled::CompiledGraph;
use super::node::{GraphNode, NodeContext};
use super::types::GraphConfig;

/// 子图节点包装器
pub struct SubgraphNode {
    node_name: String,
    subgraph: Arc<CompiledGraph>,
}

impl SubgraphNode {
    pub fn new(name: impl Into<String>, subgraph: Arc<CompiledGraph>) -> Self {
        Self {
            node_name: name.into(),
            subgraph,
        }
    }
}

#[async_trait]
impl GraphNode for SubgraphNode {
    fn name(&self) -> &str {
        &self.node_name
    }

    async fn process(
        &self,
        state: ChannelValues,
        ctx: &NodeContext,
    ) -> Result<NodeOutput, AgentError> {
        let namespace = self.node_name.clone();

        // 创建子图的 stream wrapper，自动为事件添加 namespace
        let (sub_tx, mut sub_rx) = mpsc::unbounded_channel::<StreamEvent>();

        let parent_tx = ctx.stream_tx.clone();
        let ns = namespace.clone();

        // 转发子图事件到父图（带 namespace 包装）
        tokio::spawn(async move {
            while let Some(event) = sub_rx.recv().await {
                let wrapped = StreamEvent::SubgraphEvent {
                    namespace: ns.clone(),
                    event: Box::new(event),
                };
                if parent_tx.send(wrapped).is_err() {
                    break;
                }
            }
        });

        let sub_config = GraphConfig {
            thread_id: None,
            checkpoint_id: None,
            checkpoint_ns: namespace,
            cancel: Some(ctx.cancel.clone()),
            stream_tx: Some(sub_tx),
            step_timeout: None,
            ..Default::default()
        };

        let result = self.subgraph.invoke(state, &sub_config).await?;

        match result {
            super::types::GraphResult::Done { values, .. } => Ok(NodeOutput::Update(values)),
            super::types::GraphResult::Interrupted {
                values,
                ..
            } => {
                // 子图中断冒泡到父图
                Ok(NodeOutput::Command {
                    update: Some(values),
                    goto: None,
                    graph: Some(CommandGraph::Parent),
                })
            }
        }
    }
}
