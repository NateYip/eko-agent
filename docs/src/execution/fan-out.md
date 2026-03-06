# Fan-out & Join

## Fan-out

Fan-out means one node triggers multiple parallel tasks. Each target becomes a PUSH task in the same superstep.

### Method 1: Conditional Edge Returns Send

```rust
graph.add_conditional_edge("dispatcher", |_state| {
    RouteDecision::Send(vec![
        Send::new("worker", json!({"item": "alpha"})),
        Send::new("worker", json!({"item": "beta"})),
    ])
});
```

### Method 2: Command.goto Inside Node

```rust
Ok(NodeOutput::command_with_sends(None, vec![
    Send::new("worker", json!({"id": 1})),
    Send::new("worker", json!({"id": 2})),
]))
```

Each `Send` creates a PUSH task. All PUSH tasks run in parallel in the same superstep.

## Join

Join waits for multiple nodes to finish before continuing:

```rust
graph.add_edge_join(&["a", "b"], "merge");
```

This uses `NamedBarrierValue` internally. The "merge" node runs only after both "a" and "b" have completed.
