# Retry & Timeout

## Graph-level Retry Policy

```rust
CompileConfig {
    retry_policy: Some(RetryPolicy {
        max_attempts: 3,
        initial_interval_ms: 500,
        backoff_factor: 2.0,
        max_interval_ms: 128_000,
        jitter: false,
    }),
    ..Default::default()
}
```

## Node-level Override

Implement `GraphNode` and override `retry_policy`:

```rust
fn retry_policy(&self) -> Option<&RetryPolicy> {
    Some(&self.custom_policy)
}
```

Node-level policy overrides the graph-level policy for that node.

## Retryable Errors

Only `AgentError::Llm` and `AgentError::Other` are retried. Other error variants fail immediately.

## Timeout

### Graph-level

```rust
GraphConfig {
    step_timeout: Some(Duration::from_secs(30)),
    ..Default::default()
}
```

### Node-level

```rust
fn timeout(&self) -> Option<Duration> {
    Some(Duration::from_secs(60))
}
```

Node timeout overrides graph timeout for that node.
