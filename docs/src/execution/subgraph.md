# Subgraphs

A subgraph is a compiled graph used as a node in another graph. Use this for modular, reusable workflows.

## Embedding a Subgraph

```rust
let inner = inner_graph.compile(CompileConfig::default())?;
outer_graph.add_subgraph("inner", inner);
```

The outer graph invokes the inner graph as a single node. When the outer graph reaches "inner", it runs the full inner graph to completion.

## Namespace Isolation

Subgraphs use **independent checkpoint namespaces**. Their state is stored separately from the parent graph, which avoids key collisions and simplifies debugging.

## Event Bubbling

Internal events from the subgraph surface to the parent via `SubgraphEvent`. The parent can observe or react to subgraph activity without coupling to its internals.
