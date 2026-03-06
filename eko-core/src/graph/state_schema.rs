//! 声明式 State Schema。
//!
//! 通过 `State` trait 将 Rust struct 映射为图引擎的 channel 规格，
//! 提供编译时类型安全的 state 读写。配合 `#[derive(State)]` 宏使用。

use std::collections::HashMap;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::channels::base::AnyChannel;
use crate::channels::ChannelValues;
use crate::core::error::AgentError;

/// 声明式 State 定义 trait。
///
/// 实现此 trait 的 struct 可以自动生成 channel 规格，
/// 并在 `ChannelValues`（`HashMap<String, Value>`）与强类型 struct 之间互转。
///
/// 推荐配合 `#[derive(State)]` 自动实现，而非手动编写。
///
/// # 示例
///
/// ```ignore
/// #[derive(State, Debug, Clone, Serialize, Deserialize)]
/// struct MyState {
///     pub query: String,
///
///     #[state(reducer = "my_reducer", default)]
///     pub messages: Vec<String>,
///
///     #[state(ephemeral)]
///     pub scratch: Option<String>,
/// }
/// ```
pub trait State: Sized + Send + Sync + Serialize + DeserializeOwned {
    /// 生成此 state 对应的 channel 规格（给 `StateGraph::new` 用）。
    ///
    /// 每个 struct 字段映射为一个命名 channel：
    /// - 无属性 → `LastValue`
    /// - `#[state(reducer = "...")]` → `BinaryOperatorAggregate`
    /// - `#[state(ephemeral)]` → `EphemeralValue`
    fn channels() -> HashMap<String, Box<dyn AnyChannel>>;

    /// 从 `ChannelValues` 反序列化出强类型 state。
    fn from_channel_values(values: &ChannelValues) -> Result<Self, AgentError>;

    /// 将 state 序列化为 `ChannelValues`。
    fn to_channel_values(&self) -> ChannelValues;

    /// 返回此 state 包含的所有 key 名称（字段名）。
    fn keys() -> Vec<&'static str>;
}
