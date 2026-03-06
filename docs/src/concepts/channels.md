# Channels

Channels define **how** values merge, not **what** is stored. When parallel nodes write to the same key simultaneously, the channel's merge strategy determines the final value.

| Channel Type | Merge Strategy | Typical Use |
|---|---|---|
| LastValue | Last write wins | Counters, current step |
| BinaryOperatorAggregate | Custom reducer | Message lists (append), log collection |
| EphemeralValue | Not persisted, consumed on read | One-time intermediate values |
| Topic | Append to queue, drain clears | Event queues, pending task lists |
| NamedBarrierValue | Track multi-source completion | Used internally by add_edge_join |

## Why channels?

Parallel nodes may write to the same key at the same time. Without a merge strategy, updates would conflict. Channels provide deterministic, configurable resolution.

## Reducer channel example

```rust
let mut channels = HashMap::new();
channels.insert("messages".into(), Box::new(
    BinaryOperatorAggregate::new(|old, new| {
        match old {
            Value::Array(mut arr) => { arr.push(new); Ok(Value::Array(arr)) }
            _ => Ok(Value::Array(vec![old, new])),
        }
    })
) as Box<dyn AnyChannel>);
```
