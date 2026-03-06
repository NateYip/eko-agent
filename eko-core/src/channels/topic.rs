//! Topic Channel：非持久化的 pub/sub 通道。
//!
//! 每个 superstep 开始前可由执行器清空，用于收集
//! 需要下一步并行扇出执行的 `Send` 对象。

use serde_json::Value;

use super::base::AnyChannel;

/// Topic 通道：追加式收集，非持久化，每个 superstep 可重置
pub struct Topic {
    values: Vec<Value>,
}

impl Topic {
    pub fn new() -> Self {
        Self { values: Vec::new() }
    }

    /// 清空所有收集的值（superstep 边界调用）
    pub fn drain(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.values)
    }
}

impl Default for Topic {
    fn default() -> Self {
        Self::new()
    }
}

impl AnyChannel for Topic {
    fn from_checkpoint(&self, _value: Option<Value>) -> Box<dyn AnyChannel> {
        Box::new(Topic::new())
    }

    fn update(&mut self, values: Vec<Value>) -> anyhow::Result<bool> {
        if values.is_empty() {
            return Ok(false);
        }
        self.values.extend(values);
        Ok(true)
    }

    fn get(&self) -> Option<Value> {
        if self.values.is_empty() {
            None
        } else {
            Some(Value::Array(self.values.clone()))
        }
    }

    fn checkpoint(&self) -> Option<Value> {
        None
    }

    fn is_persistent(&self) -> bool {
        false
    }
}
