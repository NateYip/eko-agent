# Superstep Execution Model

Eko executes graphs in **supersteps**. Each superstep runs a batch of tasks in parallel, then merges results and advances to the next batch.

## Superstep Loop

```
Superstep 1: [task_A, task_B]  ← parallel execution
    ↓ merge writes into channels
    ↓ save checkpoint
    ↓ resolve edges → collect next batch of tasks
Superstep 2: [task_C]
    ↓ ...
Done (no more tasks)
```

## Key Properties

- **Parallel within superstep**: All tasks in a superstep execute concurrently.
- **Batch merge**: Writes to channels are merged after the superstep completes.
- **Determinism**: Results are independent of task completion order within a superstep.
- **Checkpoint per superstep**: State is persisted after each superstep for fault tolerance.

## Flow

1. **Execute** – Run all tasks in the current batch in parallel.
2. **Merge** – Apply channel writes (reducers, last-write-wins, etc.).
3. **Checkpoint** – Persist graph state.
4. **Resolve edges** – Evaluate conditional edges and collect the next task batch.
5. **Repeat** – If tasks remain, start the next superstep; otherwise finish.
