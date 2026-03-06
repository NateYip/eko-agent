# Architecture

## Module Layout

```
eko-core/src/
├── channels/          # State channel system
│   ├── base.rs        #   AnyChannel trait
│   ├── last_value.rs  #   Last-write-wins
│   ├── binop.rs       #   Custom reducer
│   ├── ephemeral.rs   #   Temporary values (not persisted)
│   ├── topic.rs       #   Append queue
│   └── barrier.rs     #   Multi-source sync barrier
├── checkpoint/        # Checkpoint abstraction
│   ├── saver.rs       #   BaseCheckpointSaver trait
│   ├── memory.rs      #   In-memory implementation (for testing)
│   └── types.rs       #   Checkpoint / CheckpointTuple
├── core/              # Foundational contracts
│   ├── error.rs       #   AgentError / InterruptData
│   ├── llm.rs         #   LlmClient trait
│   ├── stream.rs      #   StreamEvent
│   ├── message.rs     #   Message / ToolCall
│   └── tool.rs        #   ToolDefinition trait
└── graph/             # State graph engine
    ├── builder.rs     #   StateGraph (build phase)
    ├── compiled.rs    #   CompiledGraph (execution phase)
    ├── node.rs        #   GraphNode trait / FnNode / NodeContext
    ├── command.rs     #   NodeOutput / GotoTarget / CommandGraph
    ├── edge.rs        #   Edge / RouteDecision / START / END
    ├── send.rs        #   Send (fan-out directive)
    ├── task.rs        #   Task (PULL / PUSH)
    ├── subgraph.rs    #   SubgraphNode
    └── types.rs       #   GraphResult / RetryPolicy / Command / ...
```

## Design Philosophy

`eko-core` defines **contracts (traits)** — you provide the implementations:

```
eko-core (this crate)        Your implementations            Your application
─────────────────────        ────────────────────            ────────────────
StateGraph                   ReAct / Supervisor / Swarm      Assemble & run
CompiledGraph                LlmClient impl (OpenAI, etc.)
AnyChannel                   CheckpointSaver impl (SQLite)
GraphNode trait              Tool definitions & execution
LlmClient trait              ToolExecutor
BaseCheckpointSaver trait    Session management
```

The crate ships with:

- **`MemorySaver`** — an in-memory checkpoint implementation for testing
- **`#[derive(State)]`** — a proc-macro to auto-generate channel mappings

Everything else — LLM providers, persistent storage, tool systems — lives in your code or companion crates. This keeps `eko-core` focused, dependency-light, and easy to embed in any project.
