//! 动态屏障通道。
//!
//! 与 `NamedBarrierValue` 不同，等待的节点集合在运行时通过 `WaitForNames`
//! 消息动态设定，而非编译时静态指定。
//!
//! 两个阶段：
//! - **priming**：等待 `WaitForNames` 更新来设定目标集合
//! - **waiting**：收集节点完成信号，全部到齐后 `get()` 返回 `Some`，重置回 priming

use std::collections::HashSet;

use serde_json::Value;

use super::base::AnyChannel;

/// `WaitForNames` 更新的 JSON 标记键
const WAIT_FOR_KEY: &str = "__wait_for__";

/// 动态屏障通道：运行时设定等待节点集合
pub struct DynamicBarrierValue {
    /// 等待的节点名集合，`None` 表示处于 priming 阶段
    names: Option<HashSet<String>>,
    /// 已收到完成信号的节点名
    seen: HashSet<String>,
}

impl Default for DynamicBarrierValue {
    fn default() -> Self {
        Self::new()
    }
}

impl DynamicBarrierValue {
    pub fn new() -> Self {
        Self {
            names: None,
            seen: HashSet::new(),
        }
    }
}

impl AnyChannel for DynamicBarrierValue {
    fn from_checkpoint(&self, value: Option<Value>) -> Box<dyn AnyChannel> {
        let mut ch = DynamicBarrierValue::new();
        if let Some(Value::Array(arr)) = value {
            // checkpoint 格式: [names_or_null, seen_array]
            if arr.len() == 2 {
                if let Some(names_arr) = arr[0].as_array() {
                    ch.names = Some(
                        names_arr
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect(),
                    );
                }
                if let Some(seen_arr) = arr[1].as_array() {
                    ch.seen = seen_arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
            }
        }
        Box::new(ch)
    }

    fn update(&mut self, values: Vec<Value>) -> anyhow::Result<bool> {
        let mut changed = false;
        for v in values {
            // 检测 WaitForNames 设定消息
            if let Some(obj) = v.as_object() {
                if let Some(Value::Array(names)) = obj.get(WAIT_FOR_KEY) {
                    self.names = Some(
                        names
                            .iter()
                            .filter_map(|n| n.as_str().map(String::from))
                            .collect(),
                    );
                    self.seen.clear();
                    changed = true;
                    continue;
                }
            }
            // 普通字符串 = 节点完成信号
            if let Some(name) = v.as_str() {
                if let Some(ref names) = self.names {
                    if names.contains(name) && self.seen.insert(name.to_string()) {
                        changed = true;
                    }
                }
            }
        }
        Ok(changed)
    }

    fn get(&self) -> Option<Value> {
        let names = self.names.as_ref()?;
        if !names.is_empty() && self.seen.len() >= names.len() {
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
