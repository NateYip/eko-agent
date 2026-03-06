//! 状态通道（Channel）模块。
//!
//! Channel 是状态图中节点间传递数据的抽象管道。不同类型的 Channel
//! 决定了值的更新策略：追加聚合（BinaryOperatorAggregate）、
//! 覆盖写入（LastValue）、临时传递（EphemeralValue）。

pub mod barrier;
pub mod base;
pub mod binop;
pub mod dynamic_barrier;
pub mod ephemeral;
pub mod last_value;
pub mod topic;

pub use barrier::NamedBarrierValue;
pub use base::{overwrite, AnyChannel, ChannelValues, ChannelVersion, ChannelVersions, OVERWRITE_KEY};
pub use binop::{BinaryOperatorAggregate, Reducer};
pub use dynamic_barrier::DynamicBarrierValue;
pub use ephemeral::EphemeralValue;
pub use last_value::LastValue;
pub use topic::Topic;
