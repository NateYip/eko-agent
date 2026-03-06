# State

State is the shared blackboard of the graph: a `HashMap<String, serde_json::Value>` where each key maps to a **Channel**. Nodes read from and write to this state; the engine merges updates according to each channel's strategy.

Nodes receive a **snapshot** of all channel values when they run. When a node returns `NodeOutput::Update`, those updates are merged back through each channel's merge strategy.

Example: reading and writing a counter:

```rust
// Read state
let count = state.get("counter").and_then(|v| v.as_i64()).unwrap_or(0);

// Write back
state.insert("counter".into(), json!(count + 1));
Ok(NodeOutput::Update(state))
```
