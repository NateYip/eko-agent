//! 检查点相关的数据类型定义。
//!
//! 包含 `Checkpoint`（完整快照）、`CheckpointMetadata`（元信息）、
//! `CheckpointTuple`（存储层返回的完整记录）和 `PendingWrite`（待处理写入）。

use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::channels::{ChannelValues, ChannelVersions};

/// 检查点 schema 版本号，用于未来兼容性迁移
pub const CHECKPOINT_VERSION: u32 = 1;

/// 检查点快照：记录某一时刻所有通道的值、版本号和待发送消息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub v: u32,
    pub id: String,
    pub ts: String,
    pub channel_values: ChannelValues,
    pub channel_versions: ChannelVersions,
    /// 每个节点最后一次看到的各通道版本号，用于增量更新判断
    pub versions_seen: HashMap<String, ChannelVersions>,
    #[serde(default)]
    pub pending_sends: Vec<Value>,
}

impl Checkpoint {
    /// 创建空的初始检查点
    pub fn empty() -> Self {
        Self {
            v: CHECKPOINT_VERSION,
            id: String::new(),
            ts: String::new(),
            channel_values: HashMap::new(),
            channel_versions: HashMap::new(),
            versions_seen: HashMap::new(),
            pending_sends: Vec::new(),
        }
    }
}

/// 检查点来源类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointSource {
    /// 用户输入触发
    Input,
    /// 图循环中自动生成
    Loop,
    /// 从中断恢复时生成
    Resume,
    /// 外部状态更新
    Update,
}

/// 检查点的元数据：来源、步骤号、该步的写入记录和父检查点引用
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetadata {
    pub source: CheckpointSource,
    pub step: i64,
    #[serde(default)]
    pub writes: HashMap<String, Value>,
    #[serde(default)]
    pub parents: HashMap<String, String>,
}

/// 存储层返回的完整检查点记录，包含快照、元数据和待处理写入
#[derive(Debug, Clone)]
pub struct CheckpointTuple {
    pub thread_id: String,
    pub checkpoint: Checkpoint,
    pub metadata: CheckpointMetadata,
    pub parent_checkpoint_id: Option<String>,
    pub pending_writes: Vec<PendingWrite>,
}

/// 待处理的通道写入，用于中断恢复时重放
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingWrite {
    pub task_id: String,
    pub channel: String,
    pub value: Value,
}
