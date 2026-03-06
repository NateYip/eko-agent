# eko-agent

AI Agent 的核心运行时引擎，用 Rust 实现的纯状态图执行器。

## Crates

| Crate | 描述 |
|-------|------|
| [eko-core](./eko-core/) | 状态图执行引擎：Channel、Node、Edge、Checkpoint、中断/恢复、子图 |
| [eko-core-derive](./eko-core-derive/) | `#[derive(State)]` 宏，自动生成 Channel 映射 |

## 快速开始

```toml
[dependencies]
eko-core = "0.1"
```

```rust
use eko_core::*;

let mut channels = HashMap::new();
channels.insert("msg".into(), Box::new(LastValue::new()) as Box<dyn AnyChannel>);

let mut g = StateGraph::new(channels);
g.add_node_fn("greet", |mut s, _| async move {
    s.insert("msg".into(), json!("hello world"));
    Ok(NodeOutput::Update(s))
});
g.add_edge(START, "greet");
g.add_edge("greet", END);

let compiled = g.compile(CompileConfig::default())?;
let result = compiled.invoke(HashMap::new(), &GraphConfig::default()).await?;
```

更多详情请查看 [eko-core README](./eko-core/README.md)。

## License

MIT
