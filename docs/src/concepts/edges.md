# Edges

Edges define control flow between nodes. Eko supports three types:

## Static edges

Simple unconditional transitions:

```rust
graph.add_edge("a", "b");
```

## Conditional edges

Route based on state at runtime:

```rust
graph.add_conditional_edge("router", |state| {
    if state.get("done").and_then(|v| v.as_bool()) == Some(true) {
        RouteDecision::End
    } else {
        RouteDecision::Next("worker".into())
    }
});
```

## Conditional edges with path map

Use a path map when the router returns keys that map to node names:

```rust
graph.add_conditional_edge_with_map("classifier", router_fn, path_map);
```
