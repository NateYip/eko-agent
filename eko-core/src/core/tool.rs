//! 工具定义模块。
//!
//! 描述可供 Agent 调用的外部工具，包含名称、说明和 JSON Schema 格式的参数定义。

use serde::{Deserialize, Serialize};

/// 工具定义：name 用于 LLM 函数调用，parameters 为 JSON Schema
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
