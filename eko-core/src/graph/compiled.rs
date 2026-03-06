//! 编译后的状态图运行时。
//!
//! `CompiledGraph` 是图的公共 API 入口，负责输入/输出过滤和检查点恢复，
//! 将核心执行逻辑委托给 `PregelLoop`。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use uuid::Uuid;

use crate::channels::base::{AnyChannel, ChannelValues, ChannelVersions};
use crate::checkpoint::{BaseCheckpointSaver, CheckpointSource};
use crate::core::error::AgentError;

use super::edge::Edge;
use super::node::GraphNode;
use super::pregel::channel_ops::{apply_updates, init_channels, read_channels};
use super::pregel::PregelLoop;
use super::types::{
    Command, GraphConfig, GraphResult, JoinEdge, RetryPolicy, StateSnapshot,
};

/// 编译后的不可变状态图运行时
pub struct CompiledGraph {
    /// 图名称，用于调试和子图路由
    pub name: String,
    pub(crate) pregel: PregelLoop,
    pub(crate) channel_specs: HashMap<String, Box<dyn AnyChannel>>,
    input_keys: Option<HashSet<String>>,
    output_keys: Option<HashSet<String>>,
}

impl CompiledGraph {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        channel_specs: HashMap<String, Box<dyn AnyChannel>>,
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
            name: "default".to_string(),
            pregel: PregelLoop::new(
                nodes,
                edges,
                entry,
                checkpointer,
                interrupt_before,
                interrupt_after,
                retry_policy,
                joins,
            ),
            channel_specs,
            input_keys: None,
            output_keys: None,
        }
    }

    /// 获取图名称
    pub fn graph_name(&self) -> &str {
        &self.name
    }

    /// 设置图名称
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub(crate) fn set_input_keys(&mut self, keys: Option<HashSet<String>>) {
        self.input_keys = keys;
    }

    pub(crate) fn set_output_keys(&mut self, keys: Option<HashSet<String>>) {
        self.output_keys = keys;
    }

    /// 过滤 invoke 输入：只保留 input_keys 中的字段
    fn filter_input(&self, input: ChannelValues) -> ChannelValues {
        match &self.input_keys {
            Some(keys) => input
                .into_iter()
                .filter(|(k, _)| keys.contains(k))
                .collect(),
            None => input,
        }
    }

    /// 过滤 GraphResult 中的输出值：只保留 output_keys 中的字段
    fn filter_output_result(&self, result: GraphResult) -> GraphResult {
        let keys = match &self.output_keys {
            Some(k) => k,
            None => return result,
        };
        match result {
            GraphResult::Done {
                values,
                checkpoint_id,
            } => GraphResult::Done {
                values: values
                    .into_iter()
                    .filter(|(k, _)| keys.contains(k))
                    .collect(),
                checkpoint_id,
            },
            GraphResult::Interrupted {
                values,
                checkpoint_id,
                resume_node,
                reason,
            } => GraphResult::Interrupted {
                values: values
                    .into_iter()
                    .filter(|(k, _)| keys.contains(k))
                    .collect(),
                checkpoint_id,
                resume_node,
                reason,
            },
        }
    }

    /// 从头开始执行图（如果 thread_id 关联有检查点则从最新检查点恢复状态后继续）。
    /// 若 config.checkpoint_id 指定了特定检查点，则从该检查点分支执行（用于 edit & resend）。
    pub async fn invoke(
        &self,
        input: ChannelValues,
        config: &GraphConfig,
    ) -> Result<GraphResult, AgentError> {
        let thread_id = config
            .thread_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());

        let existing = if let Some(saver) = &self.pregel.checkpointer {
            saver
                .get_tuple(&thread_id, config.checkpoint_id.as_deref())
                .await
                .map_err(|e| AgentError::GraphError(e.to_string()))?
        } else {
            None
        };

        // 初始化通道并应用用户输入
        let mut channels = init_channels(&self.channel_specs, &existing);
        let mut channel_versions: ChannelVersions = existing
            .as_ref()
            .map(|ct| ct.checkpoint.channel_versions.clone())
            .unwrap_or_default();
        let mut versions_seen: HashMap<String, ChannelVersions> = existing
            .as_ref()
            .map(|ct| ct.checkpoint.versions_seen.clone())
            .unwrap_or_default();

        let filtered_input = self.filter_input(input);
        apply_updates(&mut channels, &filtered_input, &mut channel_versions)
            .map_err(|e| AgentError::GraphError(e.to_string()))?;

        let parent_checkpoint_id = existing.as_ref().map(|ct| ct.checkpoint.id.as_str());

        let step_start: i64 = existing
            .as_ref()
            .map(|ct| ct.metadata.step + 1)
            .unwrap_or(0);

        // 保存输入阶段的初始检查点
        let initial_ckpt_id = self
            .pregel
            .save_checkpoint(
                &thread_id,
                &channels,
                &channel_versions,
                &versions_seen,
                CheckpointSource::Input,
                step_start,
                parent_checkpoint_id,
                &filtered_input,
            )
            .await
            .map_err(|e| AgentError::GraphError(e.to_string()))?;

        let start_values = read_channels(&channels);
        let entry_node = self.pregel.resolve_entry_node(&start_values)?;

        let result = self
            .pregel
            .execute(
                &thread_id,
                &mut channels,
                &mut channel_versions,
                &mut versions_seen,
                &entry_node,
                step_start + 1,
                &initial_ckpt_id,
                config,
                false,
                Vec::new(),
            )
            .await?;
        Ok(self.filter_output_result(result))
    }

    /// 恢复此前被中断的图执行，可通过 Command 注入状态更新
    pub async fn resume(
        &self,
        config: &GraphConfig,
        command: Option<Command>,
    ) -> Result<GraphResult, AgentError> {
        let thread_id = config
            .thread_id
            .as_deref()
            .ok_or_else(|| AgentError::GraphError("thread_id required for resume".into()))?;

        let saver = self
            .pregel
            .checkpointer
            .as_ref()
            .ok_or_else(|| AgentError::GraphError("Checkpointer required for resume".into()))?;

        let tuple = saver
            .get_tuple(thread_id, None)
            .await
            .map_err(|e| AgentError::GraphError(e.to_string()))?
            .ok_or_else(|| {
                AgentError::GraphError(format!("No checkpoint found for thread '{}'", thread_id))
            })?;

        let mut channels = init_channels(&self.channel_specs, &Some(tuple.clone()));
        let mut channel_versions = tuple.checkpoint.channel_versions.clone();
        let mut versions_seen = tuple.checkpoint.versions_seen.clone();

        if let Some(cmd) = &command {
            if let Some(updates) = &cmd.update {
                apply_updates(&mut channels, updates, &mut channel_versions)
                    .map_err(|e| AgentError::GraphError(e.to_string()))?;
            }
        }

        let resume_node = self.pregel.determine_resume_node(&tuple)?;
        let resume_values = PregelLoop::extract_resume_values(&tuple, &command);

        let parent_id = tuple.checkpoint.id.clone();
        let step = tuple.metadata.step + 1;

        let resume_ckpt_id = self
            .pregel
            .save_checkpoint(
                thread_id,
                &channels,
                &channel_versions,
                &versions_seen,
                CheckpointSource::Resume,
                step,
                Some(&parent_id),
                &ChannelValues::new(),
            )
            .await
            .map_err(|e| AgentError::GraphError(e.to_string()))?;

        let result = self
            .pregel
            .execute(
                thread_id,
                &mut channels,
                &mut channel_versions,
                &mut versions_seen,
                &resume_node,
                step + 1,
                &resume_ckpt_id,
                config,
                true,
                resume_values,
            )
            .await?;
        Ok(self.filter_output_result(result))
    }

    /// 获取线程的当前状态快照
    pub async fn get_state(
        &self,
        thread_id: &str,
    ) -> Result<Option<StateSnapshot>, AgentError> {
        let saver = self
            .pregel
            .checkpointer
            .as_ref()
            .ok_or_else(|| AgentError::GraphError("Checkpointer required for get_state".into()))?;

        let tuple = saver
            .get_tuple(thread_id, None)
            .await
            .map_err(|e| AgentError::GraphError(e.to_string()))?;

        let Some(tuple) = tuple else {
            return Ok(None);
        };

        let channels = init_channels(&self.channel_specs, &Some(tuple.clone()));
        let values = read_channels(&channels);

        let next = self.pregel.determine_next_nodes(&tuple);

        Ok(Some(StateSnapshot {
            values,
            next,
            created_at: tuple.checkpoint.ts.clone(),
            checkpoint: tuple,
        }))
    }

    /// 获取线程的状态历史列表
    pub async fn get_state_history(
        &self,
        thread_id: &str,
    ) -> Result<Vec<StateSnapshot>, AgentError> {
        let saver = self
            .pregel
            .checkpointer
            .as_ref()
            .ok_or_else(|| {
                AgentError::GraphError("Checkpointer required for get_state_history".into())
            })?;

        let tuples = saver
            .list(thread_id, None)
            .await
            .map_err(|e| AgentError::GraphError(e.to_string()))?;

        Ok(tuples
            .into_iter()
            .map(|tuple| {
                let channels = init_channels(&self.channel_specs, &Some(tuple.clone()));
                let values = read_channels(&channels);
                let next = self.pregel.determine_next_nodes(&tuple);
                StateSnapshot {
                    values,
                    next,
                    created_at: tuple.checkpoint.ts.clone(),
                    checkpoint: tuple,
                }
            })
            .collect())
    }

    /// 手动更新线程状态（外部注入通道值）
    pub async fn update_state(
        &self,
        thread_id: &str,
        values: ChannelValues,
        as_node: Option<&str>,
    ) -> Result<String, AgentError> {
        let saver = self
            .pregel
            .checkpointer
            .as_ref()
            .ok_or_else(|| {
                AgentError::GraphError("Checkpointer required for update_state".into())
            })?;

        let existing = saver
            .get_tuple(thread_id, None)
            .await
            .map_err(|e| AgentError::GraphError(e.to_string()))?;

        let mut channels = init_channels(&self.channel_specs, &existing);
        let mut channel_versions = existing
            .as_ref()
            .map(|ct| ct.checkpoint.channel_versions.clone())
            .unwrap_or_default();
        let mut versions_seen = existing
            .as_ref()
            .map(|ct| ct.checkpoint.versions_seen.clone())
            .unwrap_or_default();

        apply_updates(&mut channels, &values, &mut channel_versions)
            .map_err(|e| AgentError::GraphError(e.to_string()))?;

        if let Some(node_name) = as_node {
            versions_seen.insert(node_name.to_string(), channel_versions.clone());
        }

        let parent_id = existing.as_ref().map(|ct| ct.checkpoint.id.as_str());
        let step = existing
            .as_ref()
            .map(|ct| ct.metadata.step + 1)
            .unwrap_or(0);

        let ckpt_id = self
            .pregel
            .save_checkpoint(
                thread_id,
                &channels,
                &channel_versions,
                &versions_seen,
                CheckpointSource::Update,
                step,
                parent_id,
                &values,
            )
            .await
            .map_err(|e| AgentError::GraphError(e.to_string()))?;

        Ok(ckpt_id)
    }
}
