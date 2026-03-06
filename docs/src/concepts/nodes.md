# Nodes

Nodes implement the `GraphNode` trait or use `FnNode` / `add_node_fn` for closure-based nodes. Each node receives a snapshot of all channel values and a context providing `stream_tx`, cancel signal, and interrupt handling.

```rust
graph.add_node_fn("my_node", |state, ctx| async move {
    // state: snapshot of all channel values
    // ctx: provides stream_tx, cancel signal, interrupt

    let mut out = HashMap::new();
    out.insert("result".into(), json!("done"));
    Ok(NodeOutput::Update(out))
});
```

## NodeOutput variants

- **Update(values)** — Pure state update. The engine uses edges for routing; the next node is determined by the graph topology.
- **Command { goto, update }** — The node decides where to go next. Bypasses edge routing; `goto` specifies the next node directly.
