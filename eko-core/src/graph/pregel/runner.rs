//! 任务执行器。
//!
//! 负责并行执行一组 Task，支持节点级/图级重试策略和超时控制。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tracing::debug;

use crate::channels::AnyChannel;
use crate::core::error::AgentError;
use crate::core::stream::StreamEvent;
use crate::graph::command::NodeOutput;
use crate::graph::node::{GraphNode, NodeContext};
use crate::graph::task::Task;
use crate::graph::types::RetryPolicy;

use super::channel_ops::build_task_state;

/// 带重试和超时策略执行单个节点
async fn execute_node_with_policies(
    node: &dyn GraphNode,
    state: crate::channels::ChannelValues,
    ctx: &NodeContext,
    retry_policy: Option<&RetryPolicy>,
    step_timeout: Option<Duration>,
    node_name: &str,
) -> Result<NodeOutput, AgentError> {
    let execute = async {
        match retry_policy {
            Some(policy) => execute_with_retry(node, state, ctx, policy).await,
            None => node.process(state, ctx).await,
        }
    };

    match step_timeout {
        Some(duration) => tokio::time::timeout(duration, execute)
            .await
            .map_err(|_| AgentError::NodeTimeout {
                node: node_name.to_string(),
                duration,
            })?,
        None => execute.await,
    }
}

/// 按 RetryPolicy 执行节点：失败时自动重试，支持指数退避和抖动
async fn execute_with_retry(
    node: &dyn GraphNode,
    state: crate::channels::ChannelValues,
    ctx: &NodeContext,
    policy: &RetryPolicy,
) -> Result<NodeOutput, AgentError> {
    let mut attempts = 0u32;
    let mut interval = policy.initial_interval_ms;
    loop {
        match node.process(state.clone(), ctx).await {
            Ok(output) => return Ok(output),
            Err(e) if e.is_retryable() && attempts < policy.max_attempts => {
                attempts += 1;
                let sleep_ms = if policy.jitter {
                    interval + rand::random::<u64>() % interval.max(1)
                } else {
                    interval
                };
                tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                interval = ((interval as f64) * policy.backoff_factor)
                    .min(policy.max_interval_ms as f64) as u64;
            }
            Err(e) => return Err(e),
        }
    }
}

/// 并行执行一组 tasks，返回每个 task 的 NodeOutput。
/// 每个节点按优先级应用重试策略（节点级 > 图级）和超时策略（节点级 > 运行时级）。
/// `resume_values` 供节点内 `interrupt()` 恢复时使用。
pub(crate) async fn execute_tasks_parallel(
    nodes: &HashMap<String, Arc<dyn GraphNode>>,
    tasks: &[Task],
    channels: &HashMap<String, Box<dyn AnyChannel>>,
    stream_tx: &tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    cancel_rx: &tokio::sync::watch::Receiver<bool>,
    step_timeout: Option<Duration>,
    resume_values: &[serde_json::Value],
    retry_policy: Option<&RetryPolicy>,
    max_concurrency: Option<usize>,
) -> Result<Vec<NodeOutput>, AgentError> {
    if tasks.len() == 1 {
        let task = &tasks[0];
        let node = nodes.get(&task.node_name).ok_or_else(|| {
            AgentError::GraphError(format!("Node '{}' not found", task.node_name))
        })?;

        debug!(node = %task.node_name, task_type = ?task.task_type, "Executing task");

        let state = build_task_state(channels, &task.task_type);
        let ctx = NodeContext::new(stream_tx.clone(), cancel_rx.clone())
            .with_node_name(&task.node_name)
            .with_resume_values(resume_values.to_vec());

        let effective_retry = node.retry_policy().or(retry_policy);
        let effective_timeout = node.timeout().or(step_timeout);

        let output = execute_node_with_policies(
            node.as_ref(),
            state,
            &ctx,
            effective_retry,
            effective_timeout,
            &task.node_name,
        )
        .await?;
        return Ok(vec![output]);
    }

    // 多任务并行
    let mut join_set = tokio::task::JoinSet::new();
    let semaphore = max_concurrency.map(|n| Arc::new(Semaphore::new(n)));

    for task in tasks {
        let node = nodes
            .get(&task.node_name)
            .ok_or_else(|| {
                AgentError::GraphError(format!("Node '{}' not found", task.node_name))
            })?
            .clone();

        let state = build_task_state(channels, &task.task_type);

        let tx = stream_tx.clone();
        let cancel = cancel_rx.clone();
        let node_name = task.node_name.clone();
        let effective_retry = node.retry_policy().or(retry_policy).cloned();
        let effective_timeout = node.timeout().or(step_timeout);
        let rv = resume_values.to_vec();
        let sem = semaphore.clone();

        debug!(node = %node_name, task_type = ?task.task_type, "Spawning parallel task");

        join_set.spawn(async move {
            let _permit = match &sem {
                Some(s) => Some(
                    s.acquire()
                        .await
                        .map_err(|e| AgentError::GraphError(format!("Semaphore error: {e}")))?,
                ),
                None => None,
            };
            let ctx = NodeContext::new(tx, cancel)
                .with_node_name(&node_name)
                .with_resume_values(rv);
            execute_node_with_policies(
                node.as_ref(),
                state,
                &ctx,
                effective_retry.as_ref(),
                effective_timeout,
                &node_name,
            )
            .await
        });
    }

    let mut results = Vec::with_capacity(tasks.len());
    while let Some(res) = join_set.join_next().await {
        results.push(
            res.map_err(|e| AgentError::GraphError(format!("Task join error: {e}")))??
        );
    }

    Ok(results)
}
