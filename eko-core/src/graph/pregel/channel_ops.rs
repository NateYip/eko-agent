//! 通道辅助操作。
//!
//! 提供通道初始化、读取、快照和更新的纯函数，供 PregelLoop 和 CompiledGraph 共同使用。

use std::collections::HashMap;

use serde_json::Value;

use crate::channels::base::{AnyChannel, ChannelValues, ChannelVersions};
use crate::checkpoint::CheckpointTuple;
use crate::graph::task::TaskType;

/// 根据通道规格和检查点初始化所有通道实例
pub(crate) fn init_channels(
    channel_specs: &HashMap<String, Box<dyn AnyChannel>>,
    checkpoint: &Option<CheckpointTuple>,
) -> HashMap<String, Box<dyn AnyChannel>> {
    channel_specs
        .iter()
        .map(|(name, spec)| {
            let saved_value = checkpoint
                .as_ref()
                .and_then(|ct| ct.checkpoint.channel_values.get(name).cloned());
            (name.clone(), spec.from_checkpoint(saved_value))
        })
        .collect()
}

/// 收集所有需持久化通道的当前值（用于写入检查点）
pub(crate) fn snapshot_channels(
    channels: &HashMap<String, Box<dyn AnyChannel>>,
) -> ChannelValues {
    channels
        .iter()
        .filter_map(|(name, ch)| {
            if ch.is_persistent() {
                ch.checkpoint().map(|v| (name.clone(), v))
            } else {
                None
            }
        })
        .collect()
}

/// 读取所有通道的当前值（包括非持久化通道）
pub(crate) fn read_channels(
    channels: &HashMap<String, Box<dyn AnyChannel>>,
) -> ChannelValues {
    channels
        .iter()
        .filter_map(|(name, ch)| ch.get().map(|v| (name.clone(), v)))
        .collect()
}

/// 将更新值应用到对应通道，并递增已变更通道的版本号
pub(crate) fn apply_updates(
    channels: &mut HashMap<String, Box<dyn AnyChannel>>,
    updates: &ChannelValues,
    versions: &mut ChannelVersions,
) -> anyhow::Result<()> {
    for (key, value) in updates {
        if let Some(ch) = channels.get_mut(key) {
            let changed = ch.update(vec![value.clone()])?;
            if changed {
                let ver = versions.entry(key.clone()).or_insert(0);
                *ver += 1;
            }
        }
    }
    Ok(())
}

/// 将分组后的更新值批量应用到通道（支持并行任务对同一通道的多次写入）
pub(crate) fn apply_grouped_updates(
    channels: &mut HashMap<String, Box<dyn AnyChannel>>,
    grouped: &HashMap<String, Vec<Value>>,
    versions: &mut ChannelVersions,
) -> anyhow::Result<()> {
    for (key, values) in grouped {
        if let Some(ch) = channels.get_mut(key) {
            let changed = ch.update(values.clone())?;
            if changed {
                let ver = versions.entry(key.clone()).or_insert(0);
                *ver += 1;
            }
        }
    }
    Ok(())
}

/// 根据任务类型构建节点输入状态
pub(crate) fn build_task_state(
    channels: &HashMap<String, Box<dyn AnyChannel>>,
    task_type: &TaskType,
) -> ChannelValues {
    match task_type {
        TaskType::Pull => read_channels(channels),
        TaskType::Push(args) => {
            let mut state = read_channels(channels);
            if let Some(obj) = args.as_object() {
                for (k, v) in obj {
                    state.insert(k.clone(), v.clone());
                }
            }
            state
        }
    }
}
