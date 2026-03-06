# Derive Macro

The `#[derive(State)]` macro generates channel mappings from struct fields.

## Basic Usage

```rust
use eko_core::State;

#[derive(State, Debug, Clone, Serialize, Deserialize)]
struct MyState {
    // No attribute → LastValue (last-write-wins)
    counter: i64,

    // Custom reducer
    #[state(reducer = "append_messages")]
    messages: Vec<String>,

    // Not persisted
    #[state(ephemeral)]
    temp_data: Option<String>,
}

fn append_messages(old: Value, new: Value) -> Result<Value, AgentError> {
    // merge logic
}
```

## Field Attributes

| Attribute | Behavior |
|-----------|----------|
| *(none)* | `LastValue` – last write wins |
| `#[state(reducer = "fn_name")]` | Custom reducer function |
| `#[state(ephemeral)]` | Not persisted in checkpoints |

## Reducer Signature

```rust
fn append_messages(old: Value, new: Value) -> Result<Value, AgentError>
```

The reducer receives the current channel value and the incoming write, and returns the merged value.
