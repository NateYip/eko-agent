//! 检查点（Checkpoint）持久化模块。
//!
//! 负责保存和恢复状态图的执行快照，使得对话可以跨请求延续、
//! 在中断后恢复执行。core 仅提供 trait 和内存实现，
//! SQLite 等生产后端由 agent-helper 提供。

pub mod memory;
pub mod saver;
pub mod types;

pub use memory::MemorySaver;
pub use saver::BaseCheckpointSaver;
pub use types::{
    Checkpoint, CheckpointMetadata, CheckpointSource, CheckpointTuple, PendingWrite,
};
