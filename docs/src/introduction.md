# eko-core

A pure **state-graph execution engine** for building AI agents in Rust.

---

`eko-core` does one thing and does it well: **drive state through a graph of nodes and edges you define**. It contains no LLM calls, no tool implementations, no preset patterns — just the core execution engine.

## Features

| | |
|---|---|
| **Pure State Graph** | Define nodes, edges, and channels — the engine handles the rest |
| **Deterministic Execution** | Superstep model guarantees consistent results regardless of parallel task ordering |
| **Channels** | Pluggable merge strategies: last-write-wins, custom reducers, ephemeral, queues |
| **Interrupt & Resume** | Human-in-the-loop: pause execution, collect feedback, resume seamlessly |
| **Checkpoints** | Save and restore graph state at any point for fault tolerance |
| **Fan-out & Join** | Parallel task dispatch with barrier-based synchronization |
| **Subgraphs** | Compose graphs — embed a compiled graph as a node in another graph |
| **Retry & Timeout** | Per-node retry policies with exponential backoff; per-node timeouts |
| **Derive Macro** | `#[derive(State)]` auto-generates channel mappings from struct definitions |
| **Async Native** | Built on Tokio, fully async from top to bottom |

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
eko-core = "0.1"
```

## One-minute Overview

```
State  = a set of Channel values (the "shared blackboard")
Node   = reads State → does work → writes back to State
Edge   = decides which Node runs next
Graph  = Nodes + Edges + Channels
```

```rust
use eko_core::*;
use std::collections::HashMap;
use serde_json::json;

// 1. Define channels (merge strategies)
let mut channels = HashMap::new();
channels.insert("msg".into(), Box::new(LastValue::new()) as Box<dyn AnyChannel>);

// 2. Build the graph
let mut g = StateGraph::new(channels);
g.add_node_fn("greet", |mut s, _| async move {
    s.insert("msg".into(), json!("hello world"));
    Ok(NodeOutput::Update(s))
});
g.add_edge(START, "greet");
g.add_edge("greet", END);

// 3. Compile & execute
let compiled = g.compile(CompileConfig::default())?;
let result = compiled.invoke(HashMap::new(), &GraphConfig::default()).await?;
// result = GraphResult::Done { values: {"msg": "hello world"}, .. }
```

## What's Next

- **[Quick Start](./getting-started.md)** — more examples: loops, fan-out, sequences
- **[Core Concepts](./concepts/state.md)** — understand State, Channels, Nodes, and Edges
- **[Architecture](./architecture.md)** — module layout and design philosophy
