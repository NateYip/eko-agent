//! 最新值通道。
//!
//! 只保留最近一次写入的值（覆盖语义），适用于 `pending_tool_calls`、
//! `accumulated_text` 等每次节点写入都完全替换前值的字段。

use serde_json::Value;
use super::base::AnyChannel;

/// 覆盖写入通道：每次更新只保留最新的值
pub struct LastValue {
    value: Option<Value>,
}

impl Default for LastValue {
    fn default() -> Self {
        Self::new()
    }
}

impl LastValue {
    pub fn new() -> Self {
        Self { value: None }
    }
}

impl AnyChannel for LastValue {
    fn from_checkpoint(&self, value: Option<Value>) -> Box<dyn AnyChannel> {
        Box::new(LastValue { value })
    }

    fn update(&mut self, values: Vec<Value>) -> anyhow::Result<bool> {
        // 取批量更新中的最后一个值作为当前值
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
        self.value.clone()
    }
}
