//! 图执行相关的类型定义。
//!
//! 包含图执行结果、中断原因、恢复命令、编译配置、运行时配置和状态快照等。

use std::sync::Arc;
use std::time::Duration;
use serde_json::Value;
use tokio::sync::{mpsc, watch};

use std::collections::HashSet;

use crate::channels::ChannelValues;
use crate::checkpoint::{BaseCheckpointSaver, CheckpointTuple};
use crate::core::stream::{StreamEvent, StreamMode};

/// 图执行结果：正常完成或被中断
#[derive(Debug)]
pub enum GraphResult {
    /// 执行完毕，包含最终状态和检查点 ID
    Done {
        values: ChannelValues,
        checkpoint_id: String,
    },
    /// 执行被中断，包含当前状态、恢复所需信息和中断原因
    Interrupted {
        values: ChannelValues,
        checkpoint_id: String,
        resume_node: String,
        reason: InterruptReason,
    },
}

/// 中断原因枚举
#[derive(Debug, Clone)]
pub enum InterruptReason {
    /// 用户主动取消
    UserCancelled,
    /// 在指定节点执行前中断（如等待用户确认工具调用）
    InterruptBefore(String),
    /// 在指定节点执行后中断
    InterruptAfter(String),
    /// 节点内部主动中断，携带传出给用户的数据
    NodeInterrupt { value: Value },
}

/// 节点级重试策略：失败时自动重试，支持指数退避和抖动
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_interval_ms: u64,
    pub backoff_factor: f64,
    pub max_interval_ms: u64,
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_interval_ms: 500,
            backoff_factor: 2.0,
            max_interval_ms: 128_000,
            jitter: false,
        }
    }
}

/// 恢复中断时携带的命令，可包含状态更新
#[derive(Debug, Default)]
pub struct Command {
    pub resume: Option<Value>,
    pub update: Option<ChannelValues>,
}

/// 图编译配置：检查点存储、中断节点列表、图级重试策略
#[derive(Default)]
pub struct CompileConfig {
    pub checkpointer: Option<Arc<dyn BaseCheckpointSaver>>,
    pub interrupt_before: Vec<String>,
    pub interrupt_after: Vec<String>,
    pub retry_policy: Option<RetryPolicy>,
}

/// 检查点保存策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Durability {
    /// 每步同步保存检查点（默认，最安全）
    #[default]
    Sync,
    /// 每步异步保存，不阻塞下一步执行（牺牲部分恢复精度换取性能）
    Async,
    /// 仅在图执行结束时保存一次（最快，但中间步骤不可恢复）
    Exit,
}

/// 图运行时配置：线程 ID、取消信号和事件流发送端
#[derive(Default)]
pub struct GraphConfig {
    pub thread_id: Option<String>,
    /// 指定从哪个检查点开始执行（None = 最新检查点）。
    /// 用于 edit & resend 场景：从某个历史检查点分支出新的执行路径。
    pub checkpoint_id: Option<String>,
    /// 检查点命名空间，用于子图隔离。格式 `"parent|child:task_id"`
    pub checkpoint_ns: String,
    pub cancel: Option<watch::Receiver<bool>>,
    pub stream_tx: Option<mpsc::UnboundedSender<StreamEvent>>,
    /// 节点级执行超时（图级默认），各节点可通过 `GraphNode::timeout()` 覆盖
    pub step_timeout: Option<Duration>,
    /// 并行任务数量上限，None 表示不限制
    pub max_concurrency: Option<usize>,
    /// 检查点保存策略
    pub durability: Durability,
    /// 流式输出模式过滤集合。`None` 表示使用默认集合。
    pub stream_modes: Option<HashSet<StreamMode>>,
}

/// 多源边（Join）描述：等待所有 sources 完成后调度 target
#[derive(Debug, Clone)]
pub(crate) struct JoinEdge {
    pub sources: Vec<String>,
    pub target: String,
    pub channel_name: String,
}

/// 某个检查点时刻的状态快照，包含通道值和下一步可执行的节点
#[derive(Debug)]
pub struct StateSnapshot {
    pub values: ChannelValues,
    pub next: Vec<String>,
    pub checkpoint: CheckpointTuple,
    pub created_at: String,
}
