//! 临时通道。
//!
//! 每步执行后自动清空、不持久化到检查点的通道。
//! 适用于节点间的瞬时信号传递，如中间计算结果。

use serde_json::Value;
use super::base::AnyChannel;

/// 临时通道：值在每步后清空，不写入检查点
pub struct EphemeralValue {
    value: Option<Value>,
}

impl Default for EphemeralValue {
    fn default() -> Self {
        Self::new()
    }
}

impl EphemeralValue {
    pub fn new() -> Self {
        Self { value: None }
    }

    /// 取走当前值并将通道置空（move 语义）
    pub fn consume(&mut self) -> Option<Value> {
        self.value.take()
    }
}

impl AnyChannel for EphemeralValue {
    fn from_checkpoint(&self, _value: Option<Value>) -> Box<dyn AnyChannel> {
        // 临时通道恢复时始终为空
        Box::new(EphemeralValue { value: None })
    }

    fn update(&mut self, values: Vec<Value>) -> anyhow::Result<bool> {
        // 只保留最后一个更新值
        if let Some(v) = values.into_iter().last() {
            self.value = Some(v);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn get(&self) -> Option<Value> {
        self.value.clone()
    }

    fn checkpoint(&self) -> Option<Value> {
        None
    }

    fn is_persistent(&self) -> bool {
        false
    }
}
