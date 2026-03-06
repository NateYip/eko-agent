//! 检查点存储的抽象 trait 定义。
//!
//! `BaseCheckpointSaver` 定义了检查点持久化所需的全部操作接口，
//! 具体实现（内存 / SQLite）在各自模块中提供。

use async_trait::async_trait;

use super::types::{Checkpoint, CheckpointMetadata, CheckpointTuple, PendingWrite};

/// 检查点存储的抽象接口，所有后端实现都需满足此 trait
#[async_trait]
pub trait BaseCheckpointSaver: Send + Sync {
    /// 加载线程的最新检查点，或按 checkpoint_id 加载特定检查点
    async fn get_tuple(
        &self,
        thread_id: &str,
        checkpoint_id: Option<&str>,
    ) -> anyhow::Result<Option<CheckpointTuple>>;

    /// 保存检查点，返回生成的 checkpoint_id
    async fn put(
        &self,
        thread_id: &str,
        checkpoint: Checkpoint,
        metadata: CheckpointMetadata,
    ) -> anyhow::Result<String>;

    /// 保存某个检查点关联的待处理写入
    async fn put_writes(
        &self,
        thread_id: &str,
        checkpoint_id: &str,
        writes: Vec<PendingWrite>,
        task_id: &str,
    ) -> anyhow::Result<()>;

    /// 列出线程的检查点，按时间倒序排列
    async fn list(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<CheckpointTuple>>;

    /// 删除线程的所有检查点和待处理写入
    async fn delete_thread(&self, thread_id: &str) -> anyhow::Result<()>;
}
