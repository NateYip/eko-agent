//! Pregel 执行引擎核心。
//!
//! `PregelLoop` 实现基于 superstep 的图执行循环，负责：
//! - 按步调度节点任务（PULL / PUSH）
//! - 处理 interrupt_before / interrupt_after 中断
//! - 节点内 interrupt() 的保存与恢复
//! - 每步保存检查点
//! - 解析边 / Command 路由下一步任务

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::channels::base::{AnyChannel, ChannelValues, ChannelVersions};
use crate::checkpoint::{
    BaseCheckpointSaver, Checkpoint, CheckpointMetadata, CheckpointSource, CheckpointTuple,
    PendingWrite,
};
use crate::core::error::AgentError;
use crate::core::stream::{StreamEvent, StreamMode};
use crate::graph::command::{CommandGraph, GotoTarget, NodeOutput};
use crate::graph::edge::{Edge, RouteDecision, START};
use crate::graph::node::GraphNode;
use crate::graph::task::Task;
use crate::graph::types::{
    Command, Durability, GraphConfig, GraphResult, InterruptReason, JoinEdge, RetryPolicy,
};

use super::channel_ops::{apply_grouped_updates, read_channels, snapshot_channels};
use super::runner::execute_tasks_parallel;

/// 单次 invoke/resume 的最大 superstep 数上限，防止无限循环
pub(crate) const MAX_SUPERSTEPS: usize = 100;

/// 如果事件对应的 StreamMode 在 active_modes 中则发送，否则丢弃
fn send_if_active(
    tx: &tokio::sync::mpsc::UnboundedSender<StreamEvent>,
    event: StreamEvent,
    active_modes: &std::collections::HashSet<StreamMode>,
) {
    if let Some(mode) = event.mode() {
        if active_modes.contains(&mode) {
            let _ = tx.send(event);
        }
    } else {
        let _ = tx.send(event);
    }
}

/// Pregel 执行引擎：基于 superstep 的消息传递模型
pub(crate) struct PregelLoop {
    nodes: HashMap<String, Arc<dyn GraphNode>>,
    edges: HashMap<String, Edge>,
    joins: Vec<JoinEdge>,
    interrupt_before: HashSet<String>,
    interrupt_after: HashSet<String>,
    retry_policy: Option<RetryPolicy>,
    pub(crate) checkpointer: Option<Arc<dyn BaseCheckpointSaver>>,
    entry: String,
}

impl PregelLoop {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        nodes: HashMap<String, Arc<dyn GraphNode>>,
        edges: HashMap<String, Edge>,
        entry: String,
        checkpointer: Option<Arc<dyn BaseCheckpointSaver>>,
        interrupt_before: HashSet<String>,
        interrupt_after: HashSet<String>,
        retry_policy: Option<RetryPolicy>,
        joins: Vec<JoinEdge>,
    ) -> Self {
        Self {
            nodes,
            edges,
            joins,
            interrupt_before,
            interrupt_after,
            retry_policy,
            checkpointer,
            entry,
        }
    }
}

impl PregelLoop {
    /// Superstep 执行循环：
    /// 1. 收集当前 superstep 的所有 tasks (PULL + PUSH)
    /// 2. 并行执行所有 tasks
    /// 3. 合并 writes 并应用到 channels
    /// 4. 保存 checkpoint
    /// 5. 处理 interrupt（含节点内 interrupt）
    /// 6. 解析边/Command -> 收集下一 superstep 的 tasks
    /// 7. 若无 tasks -> 结束
    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        &self,
        thread_id: &str,
        channels: &mut HashMap<String, Box<dyn AnyChannel>>,
        channel_versions: &mut ChannelVersions,
        versions_seen: &mut HashMap<String, ChannelVersions>,
        start_node: &str,
        start_step: i64,
        parent_ckpt_id: &str,
        config: &GraphConfig,
        is_resuming: bool,
        resume_values: Vec<serde_json::Value>,
    ) -> Result<GraphResult, AgentError> {
        let mut step = start_step;
        let mut last_ckpt_id = parent_ckpt_id.to_string();
        let mut first_iteration = is_resuming;
        let mut current_resume_values = resume_values;

        let (default_tx, _) = tokio::sync::mpsc::unbounded_channel();
        let (default_cancel_tx, default_cancel_rx) = tokio::sync::watch::channel(false);
        drop(default_cancel_tx);

        let stream_tx = config.stream_tx.as_ref().unwrap_or(&default_tx);
        let cancel_rx = config.cancel.as_ref().unwrap_or(&default_cancel_rx);
        let default_modes = StreamMode::default_set();
        let active_modes = config.stream_modes.as_ref().unwrap_or(&default_modes);

        let mut pending_tasks = vec![Task::pull(start_node)];

        for _ in 0..MAX_SUPERSTEPS {
            if pending_tasks.is_empty() {
                // Durability::Exit — 在图执行完成时保存最终检查点
                if config.durability == Durability::Exit {
                    last_ckpt_id = self
                        .save_checkpoint(
                            thread_id,
                            channels,
                            channel_versions,
                            versions_seen,
                            CheckpointSource::Loop,
                            step,
                            Some(&last_ckpt_id),
                            &read_channels(channels),
                        )
                        .await
                        .map_err(|e| AgentError::GraphError(e.to_string()))?;
                }
                let values = read_channels(channels);
                let _ = stream_tx.send(StreamEvent::Done {
                    thread_id: thread_id.to_string(),
                });
                return Ok(GraphResult::Done {
                    values,
                    checkpoint_id: last_ckpt_id,
                });
            }

            if *cancel_rx.borrow() {
                let values = read_channels(channels);
                let resume_node = pending_tasks
                    .first()
                    .map(|t| t.node_name.clone())
                    .unwrap_or_default();
                return Ok(GraphResult::Interrupted {
                    checkpoint_id: last_ckpt_id,
                    values,
                    resume_node,
                    reason: InterruptReason::UserCancelled,
                });
            }

            // interrupt_before 检查（仅对 PULL 任务的第一个节点）
            if !first_iteration {
                if let Some(task) = pending_tasks.first() {
                    if self.interrupt_before.contains(&task.node_name) {
                        let values = read_channels(channels);
                        let node_name = task.node_name.clone();

                        if let Some(saver) = &self.checkpointer {
                            let pw = vec![PendingWrite {
                                task_id: "interrupt".to_string(),
                                channel: node_name.clone(),
                                value: serde_json::Value::Null,
                            }];
                            let _ = saver
                                .put_writes(thread_id, &last_ckpt_id, pw, "interrupt")
                                .await;
                        }

                        return Ok(GraphResult::Interrupted {
                            checkpoint_id: last_ckpt_id,
                            values,
                            resume_node: node_name.clone(),
                            reason: InterruptReason::InterruptBefore(node_name),
                        });
                    }
                }
            }
            first_iteration = false;

            // 发送 TaskStarted 事件
            for task in &pending_tasks {
                send_if_active(
                    stream_tx,
                    StreamEvent::TaskStarted { node: task.node_name.clone() },
                    active_modes,
                );
            }

            let task_results = match execute_tasks_parallel(
                &self.nodes,
                &pending_tasks,
                channels,
                stream_tx,
                cancel_rx,
                config.step_timeout,
                &current_resume_values,
                self.retry_policy.as_ref(),
                config.max_concurrency,
            )
            .await
            {
                Ok(results) => results,
                Err(AgentError::Interrupt(data)) => {
                    // 节点内主动中断：保存中断信息和已有 resume 值到 checkpoint
                    if let Some(saver) = &self.checkpointer {
                        let pw = vec![
                            PendingWrite {
                                task_id: "interrupt".to_string(),
                                channel: "__interrupt__".to_string(),
                                value: serde_json::json!({
                                    "index": data.index,
                                    "value": data.value,
                                    "node_name": data.node_name,
                                }),
                            },
                            PendingWrite {
                                task_id: "interrupt".to_string(),
                                channel: "__resume__".to_string(),
                                value: serde_json::Value::Array(current_resume_values.clone()),
                            },
                        ];
                        let _ = saver
                            .put_writes(thread_id, &last_ckpt_id, pw, "interrupt")
                            .await;
                    }

                    let values = read_channels(channels);
                    return Ok(GraphResult::Interrupted {
                        checkpoint_id: last_ckpt_id,
                        values,
                        resume_node: data.node_name.clone(),
                        reason: InterruptReason::NodeInterrupt { value: data.value },
                    });
                }
                Err(e) => return Err(e),
            };

            // 后续迭代不再携带 resume_values
            current_resume_values = Vec::new();

            // 收集所有 task 的 writes（同一 channel 可能有多个并行写入）
            let mut grouped_updates: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
            let mut task_outputs: Vec<(String, NodeOutput)> = Vec::new();
            let mut parent_command: Option<NodeOutput> = None;

            for (task, output) in pending_tasks.iter().zip(task_results) {
                if let NodeOutput::Command {
                    graph: Some(CommandGraph::Parent),
                    ..
                } = &output
                {
                    parent_command = Some(output);
                    continue;
                }

                let updates = match &output {
                    NodeOutput::Update(v) => v.clone(),
                    NodeOutput::Command { update, .. } => update.clone().unwrap_or_default(),
                };

                for (k, v) in updates {
                    grouped_updates.entry(k).or_default().push(v);
                }

                // 发送 StateUpdate + TaskCompleted
                let node_updates: HashMap<String, serde_json::Value> = match &output {
                    NodeOutput::Update(v) => v.clone(),
                    NodeOutput::Command { update, .. } => update.clone().unwrap_or_default(),
                };
                send_if_active(
                    stream_tx,
                    StreamEvent::StateUpdate {
                        node: task.node_name.clone(),
                        updates: node_updates,
                    },
                    active_modes,
                );
                send_if_active(
                    stream_tx,
                    StreamEvent::TaskCompleted { node: task.node_name.clone() },
                    active_modes,
                );

                versions_seen.insert(task.node_name.clone(), channel_versions.clone());
                task_outputs.push((task.node_name.clone(), output));
            }

            apply_grouped_updates(channels, &grouped_updates, channel_versions)
                .map_err(|e| AgentError::GraphError(e.to_string()))?;

            let all_updates: ChannelValues = grouped_updates
                .iter()
                .filter_map(|(k, vals)| vals.last().map(|v| (k.clone(), v.clone())))
                .collect();

            // 若有 PARENT 命令，中断当前图执行并返回
            if let Some(parent_cmd) = parent_command {
                let values = read_channels(channels);
                let _goto = parent_cmd.goto().map(String::from);
                return Ok(GraphResult::Done {
                    values,
                    checkpoint_id: last_ckpt_id,
                });
            }

            match config.durability {
                Durability::Sync => {
                    last_ckpt_id = self
                        .save_checkpoint(
                            thread_id,
                            channels,
                            channel_versions,
                            versions_seen,
                            CheckpointSource::Loop,
                            step,
                            Some(&last_ckpt_id),
                            &all_updates,
                        )
                        .await
                        .map_err(|e| AgentError::GraphError(e.to_string()))?;
                }
                Durability::Async => {
                    // Fire-and-forget: checkpoint 在后台保存，不阻塞下一步
                    last_ckpt_id = Uuid::new_v4().to_string();
                    if let Some(saver) = &self.checkpointer {
                        let saver = saver.clone();
                        let tid = thread_id.to_string();
                        let ckpt_id = last_ckpt_id.clone();
                        let ch_vals = snapshot_channels(channels);
                        let ch_vers = channel_versions.clone();
                        let vs = versions_seen.clone();
                        let parent = last_ckpt_id.clone();
                        let writes = all_updates.clone();
                        tokio::spawn(async move {
                            let checkpoint = Checkpoint {
                                v: 1,
                                id: ckpt_id,
                                ts: Utc::now().to_rfc3339(),
                                channel_values: ch_vals,
                                channel_versions: ch_vers,
                                versions_seen: vs,
                                pending_sends: Vec::new(),
                            };
                            let metadata = CheckpointMetadata {
                                source: CheckpointSource::Loop,
                                step,
                                writes,
                                parents: HashMap::from([(String::new(), parent)]),
                            };
                            let _ = saver.put(&tid, checkpoint, metadata).await;
                        });
                    }
                }
                Durability::Exit => {
                    // 延迟到循环结束再保存，这里只更新 ID
                    last_ckpt_id = Uuid::new_v4().to_string();
                }
            }

            // 发送 StateValues（每步结束后的完整状态）和 CheckpointSaved
            send_if_active(
                stream_tx,
                StreamEvent::StateValues {
                    values: read_channels(channels),
                    step,
                },
                active_modes,
            );
            if !last_ckpt_id.is_empty() {
                send_if_active(
                    stream_tx,
                    StreamEvent::CheckpointSaved {
                        checkpoint_id: last_ckpt_id.clone(),
                        step,
                    },
                    active_modes,
                );
            }

            // interrupt_after 检查
            for (node_name, _) in &task_outputs {
                if self.interrupt_after.contains(node_name) {
                    let values = read_channels(channels);
                    return Ok(GraphResult::Interrupted {
                        checkpoint_id: last_ckpt_id,
                        values,
                        resume_node: node_name.clone(),
                        reason: InterruptReason::InterruptAfter(node_name.clone()),
                    });
                }
            }

            // 收集下一 superstep 的 tasks
            pending_tasks = self.prepare_next_tasks(channels, &task_outputs)?;

            step += 1;
        }

        Err(AgentError::RecursionLimit {
            limit: MAX_SUPERSTEPS,
            step: step as usize,
        })
    }

    /// 根据边定义解析从 from 节点出发的下一个目标节点
    pub(crate) fn resolve_next(&self, from: &str, values: &ChannelValues) -> Option<String> {
        match self.edges.get(from)? {
            Edge::Static(to) => Some(to.clone()),
            Edge::Conditional(router) => match router(values) {
                RouteDecision::Next(n) => Some(n),
                RouteDecision::End => None,
                RouteDecision::Send(_) => None,
            },
            Edge::End => None,
        }
    }

    /// 解析实际入口节点：若 entry 是 START 占位符，通过条件边路由决定
    pub(crate) fn resolve_entry_node(&self, values: &ChannelValues) -> Result<String, AgentError> {
        if self.entry == START {
            self.resolve_next(START, values).ok_or_else(|| {
                AgentError::GraphError("START conditional edge resolved to END".into())
            })
        } else {
            Ok(self.entry.clone())
        }
    }

    /// 根据检查点中的 pending_writes 和 metadata 判断恢复执行的起始节点
    pub(crate) fn determine_resume_node(
        &self,
        tuple: &CheckpointTuple,
    ) -> Result<String, AgentError> {
        // 节点内 interrupt()：从 __interrupt__ 写入中恢复到中断节点
        for write in &tuple.pending_writes {
            if write.channel == "__interrupt__" {
                if let Some(node_name) = write.value.get("node_name").and_then(|v| v.as_str()) {
                    return Ok(node_name.to_string());
                }
            }
        }

        for write in &tuple.pending_writes {
            if self.interrupt_before.contains(&write.channel) {
                return Ok(write.channel.clone());
            }
            if self.interrupt_after.contains(&write.channel) {
                if let Some(next) =
                    self.resolve_next(&write.channel, &tuple.checkpoint.channel_values)
                {
                    return Ok(next);
                }
            }
        }

        if let Some(last_node) = tuple.metadata.writes.keys().next() {
            if let Some(next) =
                self.resolve_next(last_node, &tuple.checkpoint.channel_values)
            {
                return Ok(next);
            }
        }

        Ok(self.entry.clone())
    }

    /// 从 checkpoint 的 pending_writes 中提取已有的 resume 值，
    /// 并追加 Command.resume 中的新值（如有）
    pub(crate) fn extract_resume_values(
        tuple: &CheckpointTuple,
        command: &Option<Command>,
    ) -> Vec<serde_json::Value> {
        let mut values: Vec<serde_json::Value> = tuple
            .pending_writes
            .iter()
            .find(|pw| pw.channel == "__resume__")
            .and_then(|pw| pw.value.as_array().cloned())
            .unwrap_or_default();

        if let Some(cmd) = command {
            if let Some(resume_val) = &cmd.resume {
                values.push(resume_val.clone());
            }
        }
        values
    }

    /// 根据检查点中最后写入的节点，推断接下来可执行的节点列表
    pub(crate) fn determine_next_nodes(&self, tuple: &CheckpointTuple) -> Vec<String> {
        if let Some(last_node) = tuple.metadata.writes.keys().next() {
            if let Some(next) =
                self.resolve_next(last_node, &tuple.checkpoint.channel_values)
            {
                return vec![next];
            }
        }
        Vec::new()
    }

    /// 将当前通道状态持久化为一个新的检查点
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn save_checkpoint(
        &self,
        thread_id: &str,
        channels: &HashMap<String, Box<dyn AnyChannel>>,
        channel_versions: &ChannelVersions,
        versions_seen: &HashMap<String, ChannelVersions>,
        source: CheckpointSource,
        step: i64,
        parent_id: Option<&str>,
        writes: &ChannelValues,
    ) -> anyhow::Result<String> {
        let Some(saver) = &self.checkpointer else {
            return Ok(String::new());
        };

        let checkpoint_id = Uuid::new_v4().to_string();
        let checkpoint = Checkpoint {
            v: 1,
            id: checkpoint_id.clone(),
            ts: Utc::now().to_rfc3339(),
            channel_values: snapshot_channels(channels),
            channel_versions: channel_versions.clone(),
            versions_seen: versions_seen.clone(),
            pending_sends: Vec::new(),
        };

        let mut parents = HashMap::new();
        if let Some(pid) = parent_id {
            parents.insert(String::new(), pid.to_string());
        }

        let metadata = CheckpointMetadata {
            source,
            step,
            writes: writes.clone(),
            parents,
        };

        saver.put(thread_id, checkpoint, metadata).await
    }

    /// 根据 task 输出解析边/Command/Barrier，收集下一 superstep 的 tasks
    fn prepare_next_tasks(
        &self,
        channels: &mut HashMap<String, Box<dyn AnyChannel>>,
        task_outputs: &[(String, NodeOutput)],
    ) -> Result<Vec<Task>, AgentError> {
        let mut next_tasks = Vec::new();
        let values = read_channels(channels);

        // 构建 join source 查询表：节点名 -> 参与的所有 JoinEdge
        let mut join_lookup: HashMap<&str, Vec<&JoinEdge>> = HashMap::new();
        for join in &self.joins {
            for source in &join.sources {
                join_lookup
                    .entry(source.as_str())
                    .or_default()
                    .push(join);
            }
        }
        let mut satisfied_joins: HashSet<String> = HashSet::new();

        for (node_name, output) in task_outputs {
            // Command.goto 优先：支持单节点跳转和并行扇出
            if let Some(goto_target) = output.goto_target() {
                match goto_target {
                    GotoTarget::Node(name) => {
                        next_tasks.push(Task::pull(name));
                    }
                    GotoTarget::Sends(sends) => {
                        for s in sends {
                            next_tasks.push(Task::push(&s.node, s.args.clone()));
                        }
                    }
                }
                continue;
            }

            // 检查节点是否为 join source：更新 barrier 并在满足时调度 target
            if let Some(joins) = join_lookup.get(node_name.as_str()) {
                for join in joins {
                    if let Some(ch) = channels.get_mut(&join.channel_name) {
                        let _ = ch.update(vec![serde_json::json!(node_name)]);
                    }
                    if let Some(ch) = channels.get(&join.channel_name) {
                        if ch.get().is_some()
                            && satisfied_joins.insert(join.channel_name.clone())
                        {
                            next_tasks.push(Task::pull(&join.target));
                            let new_ch = ch.from_checkpoint(None);
                            channels.insert(join.channel_name.clone(), new_ch);
                        }
                    }
                }
                continue;
            }

            let edge = match self.edges.get(node_name) {
                Some(e) => e,
                None => {
                    return Err(AgentError::UnreachableNode(node_name.clone()));
                }
            };

            match edge {
                Edge::Static(next) => {
                    next_tasks.push(Task::pull(next.as_str()));
                }
                Edge::Conditional(router) => match router(&values) {
                    RouteDecision::Next(next) => {
                        next_tasks.push(Task::pull(next));
                    }
                    RouteDecision::End => {}
                    RouteDecision::Send(sends) => {
                        for s in sends {
                            next_tasks.push(Task::push(&s.node, s.args.clone()));
                        }
                    }
                },
                Edge::End => {}
            }
        }

        Ok(next_tasks)
    }
}
