# Quick Start

## Lifecycle: Build → Compile → Execute

Every graph follows three phases:

```rust
// 1. Build (mutable)
let mut graph = StateGraph::new(channels);
graph.add_node_fn("a", ...);
graph.add_edge(START, "a");
graph.add_edge("a", END);

// 2. Compile (immutable) — validates topology
let compiled = graph.compile(CompileConfig {
    checkpointer: Some(saver),
    interrupt_before: vec!["tools".into()],
    retry_policy: Some(RetryPolicy::default()),
    ..Default::default()
})?;

// 3. Execute
let result = compiled.invoke(input, &GraphConfig {
    thread_id: Some("conversation-1".into()),
    step_timeout: Some(Duration::from_secs(30)),
    ..Default::default()
}).await?;
```

## Example: Simple Linear Graph

```rust
use eko_core::*;
use std::collections::HashMap;
use serde_json::json;

let mut channels = HashMap::new();
channels.insert("msg".into(), Box::new(LastValue::new()) as Box<dyn AnyChannel>);

let mut g = StateGraph::new(channels);
g.add_node_fn("greet", |mut s, _| async move {
    s.insert("msg".into(), json!("hello world"));
    Ok(NodeOutput::Update(s))
});
g.add_edge(START, "greet");
g.add_edge("greet", END);

let compiled = g.compile(CompileConfig::default())?;
let result = compiled.invoke(HashMap::new(), &GraphConfig::default()).await?;
// result = GraphResult::Done { values: {"msg": "hello world"}, .. }
```

## Example: Conditional Loop

```rust
g.add_node_fn("increment", |mut s, _| async move {
    let v = s.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
    s.insert("count".into(), json!(v + 1));
    Ok(NodeOutput::Update(s))
});
g.add_edge(START, "increment");
g.add_conditional_edge("increment", |state| {
    let count = state.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
    if count >= 5 {
        RouteDecision::End
    } else {
        RouteDecision::Next("increment".into())
    }
});
```

## Example: Parallel Fan-out + Join

```rust
g.add_node_fn("dispatcher", |_s, _| async move {
    Ok(NodeOutput::Update(HashMap::new()))
});
g.add_node_fn("worker_a", |_s, _| async move { /* ... */ });
g.add_node_fn("worker_b", |_s, _| async move { /* ... */ });
g.add_node_fn("merger",   |_s, _| async move { /* ... */ });

g.add_edge(START, "dispatcher");
g.add_conditional_edge("dispatcher", |_| RouteDecision::Send(vec![
    Send::new("worker_a", json!({})),
    Send::new("worker_b", json!({})),
]));
g.add_edge_join(&["worker_a", "worker_b"], "merger");
g.add_edge("merger", END);
```

## Example: Sequence Helper

```rust
g.add_sequence(vec![
    Box::new(FnNode::new("step1", |s, _| async move { /* ... */ })),
    Box::new(FnNode::new("step2", |s, _| async move { /* ... */ })),
    Box::new(FnNode::new("step3", |s, _| async move { /* ... */ })),
]);
g.add_edge(START, "step1");
g.add_edge("step3", END);
```
