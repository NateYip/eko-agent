//! 二元聚合通道。
//!
//! 通过用户提供的 reducer 函数将新值与已有值合并，而非覆盖。
//! 典型用途是 `messages` 通道：每次节点输出的新消息追加到消息列表末尾。

use std::sync::Arc;
use serde_json::Value;
use super::base::{AnyChannel, OVERWRITE_KEY};

/// Reducer 函数类型：接收已有值和更新值，返回合并后的新值
pub type Reducer = Arc<dyn Fn(Value, Value) -> anyhow::Result<Value> + Send + Sync>;

/// 使用二元 reducer 函数聚合值的通道，适用于需要累积（如追加消息列表）的场景
pub struct BinaryOperatorAggregate {
    value: Option<Value>,
    reducer: Reducer,
}

impl BinaryOperatorAggregate {
    /// 创建聚合通道，传入自定义的合并函数
    pub fn new(reducer: impl Fn(Value, Value) -> anyhow::Result<Value> + Send + Sync + 'static) -> Self {
        Self {
            value: None,
            reducer: Arc::new(reducer),
        }
    }
}

impl AnyChannel for BinaryOperatorAggregate {
    fn from_checkpoint(&self, value: Option<Value>) -> Box<dyn AnyChannel> {
        Box::new(BinaryOperatorAggregate {
            value,
            reducer: self.reducer.clone(),
        })
    }

    fn update(&mut self, values: Vec<Value>) -> anyhow::Result<bool> {
        if values.is_empty() {
            return Ok(false);
        }
        let mut changed = false;
        for v in values {
            // Overwrite 标记：绕过 reducer 直接覆盖
            if let Some(obj) = v.as_object() {
                if let Some(actual) = obj.get(OVERWRITE_KEY) {
                    self.value = Some(actual.clone());
                    changed = true;
                    continue;
                }
            }
            match &self.value {
                Some(existing) => {
                    self.value = Some((self.reducer)(existing.clone(), v)?);
                    changed = true;
                }
                None => {
                    self.value = Some(v);
                    changed = true;
                }
            }
        }
        Ok(changed)
    }

    fn get(&self) -> Option<Value> {
        self.value.clone()
    }

    fn checkpoint(&self) -> Option<Value> {
        self.value.clone()
    }
}
