//! Channel 基础特征与类型定义。
//!
//! 定义了状态通道的核心 trait `AnyChannel` 以及在图中流转状态时使用的类型别名。
//! 所有通道值通过 `serde_json::Value` 进行动态分发，节点边界处再转换为强类型。

use std::collections::HashMap;
use serde_json::Value;

/// Overwrite 标记键：当 update 值为 `{"__overwrite__": actual_value}` 时，
/// 带 reducer 的通道会跳过 reducer 直接覆盖为 `actual_value`。
pub const OVERWRITE_KEY: &str = "__overwrite__";

/// 构造 Overwrite 标记值，用于绕过 reducer 直接覆盖通道值。
pub fn overwrite(value: Value) -> Value {
    serde_json::json!({ OVERWRITE_KEY: value })
}

pub type ChannelVersion = u64;
pub type ChannelValues = HashMap<String, Value>;
pub type ChannelVersions = HashMap<String, ChannelVersion>;

/// 类型擦除的通道接口，所有 Channel 实现均需满足此 trait。
/// 通过 `serde_json::Value` 实现动态分发，节点内部再做强类型转换。
#[allow(clippy::wrong_self_convention)]
pub trait AnyChannel: Send + Sync {
    /// 从检查点值创建新的通道实例，`None` 表示全新开始
    fn from_checkpoint(&self, value: Option<Value>) -> Box<dyn AnyChannel>;

    /// 应用一批更新值，返回通道值是否发生了变化
    fn update(&mut self, values: Vec<Value>) -> anyhow::Result<bool>;

    /// 获取当前通道值，通道为空时返回 `None`
    fn get(&self) -> Option<Value>;

    /// 获取需要持久化到检查点的值
    fn checkpoint(&self) -> Option<Value>;

    /// 该通道是否需要持久化到检查点，默认为 true
    fn is_persistent(&self) -> bool {
        true
    }
}
