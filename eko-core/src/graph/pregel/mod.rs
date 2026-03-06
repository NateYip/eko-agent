//! Pregel 执行引擎模块。
//!
//! 基于 superstep 的消息传递执行模型，将图的执行逻辑从 CompiledGraph 中解耦。
//! - `PregelLoop`：superstep 循环 + 中断处理 + checkpoint 保存
//! - `runner`：并行任务执行 + 重试 + 超时
//! - `channel_ops`：通道初始化 / 读取 / 快照 / 更新

pub(crate) mod channel_ops;
pub(crate) mod engine;
pub(crate) mod runner;

pub(crate) use engine::PregelLoop;
