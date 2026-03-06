# Interrupt & Resume

Eko supports three ways to interrupt execution and resume later.

## 1. Interrupt Before Node (Compile-time)

```rust
CompileConfig {
    interrupt_before: vec!["dangerous_node".into()],
    ..
}
```

Execution stops before entering the specified node. Resume to continue.

## 2. Interrupt After Node (Compile-time)

```rust
CompileConfig {
    interrupt_after: vec!["review_node".into()],
    ..
}
```

Execution stops after the node completes. Useful for review gates.

## 3. Runtime Interrupt Inside Node

Most flexible: interrupt from within node logic and pass data to the caller.

```rust
graph.add_node_fn("planner", |mut state, ctx| async move {
    let plan = create_plan(&state);
    let feedback = ctx.interrupt(json!({"plan": plan, "question": "Confirm?"}))?;
    execute_plan(&plan, &feedback);
    Ok(NodeOutput::Update(state))
});
```

## Resuming

```rust
compiled.resume(&config, Some(Command {
    resume: Some(json!("approved")),
    update: None,
})).await?;
```

The `resume` value is passed to the node that called `ctx.interrupt()`, allowing human-in-the-loop flows.
