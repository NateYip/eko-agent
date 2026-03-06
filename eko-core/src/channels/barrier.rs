//! Barrier 通道：多源边同步屏障。
//!
//! 当所有预设的源节点都完成时，`get()` 才返回 `Some`，
//! 用于实现 `add_edge_join([A, B], C)` 的等待逻辑。

use std::collections::HashSet;

use serde_json::Value;

use super::base::AnyChannel;

/// 多源同步屏障通道：追踪一组节点的完成状态
pub struct NamedBarrierValue {
    names: HashSet<String>,
    seen: HashSet<String>,
}

impl NamedBarrierValue {
    pub fn new(names: &[&str]) -> Self {
        Self {
            names: names.iter().map(|n| n.to_string()).collect(),
            seen: HashSet::new(),
        }
    }
}

impl AnyChannel for NamedBarrierValue {
    fn from_checkpoint(&self, _value: Option<Value>) -> Box<dyn AnyChannel> {
        Box::new(NamedBarrierValue {
            names: self.names.clone(),
            seen: HashSet::new(),
        })
    }

    fn update(&mut self, values: Vec<Value>) -> anyhow::Result<bool> {
        let mut changed = false;
        for v in values {
            if let Some(name) = v.as_str() {
                if self.names.contains(name) && self.seen.insert(name.to_string()) {
                    changed = true;
                }
            }
        }
        Ok(changed)
    }

    fn get(&self) -> Option<Value> {
        if !self.names.is_empty() && self.seen.len() == self.names.len() {
            Some(Value::Bool(true))
        } else {
            None
        }
    }

    fn checkpoint(&self) -> Option<Value> {
        None
    }

    fn is_persistent(&self) -> bool {
        false
    }
}
