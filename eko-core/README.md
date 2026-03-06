# eko-core

AI Agent 的核心运行时引擎 —— 一个纯粹的**状态图执行器**。

本库不包含任何具体的 LLM 调用、工具实现或预置模式（ReAct / Supervisor / Swarm），
这些由上层库提供。`eko-core` 只关心一件事：
**按照你定义的图结构，驱动状态在节点之间流转。**

## 核心概念

### 一句话理解

```
State（状态）= 一组 Channel 的当前值
Node（节点）= 读取 State → 做点事 → 写回 State
Edge（边）   = 决定下一个执行哪个 Node
Graph（图）  = Node + Edge + Channel 的组合
```

### State —— "共享黑板"

State 不是一个特殊的结构体，它就是 `HashMap<String, serde_json::Value>`。
每个 key 对应一个 **Channel**（通道），value 是该通道的当前值。

节点执行时收到的 `state` 是所有通道的当前快照；
节点返回的 `NodeOutput` 中的更新会通过各通道的合并策略写回。

```rust
// 节点读取 state
let count = state.get("counter").and_then(|v| v.as_i64()).unwrap_or(0);

// 节点写回 state
state.insert("counter".into(), json!(count + 1));
Ok(NodeOutput::Update(state))
```

### Channel —— "值怎么合并"

Channel 定义的不是"存什么"，而是**"多次写入时怎么合并"**。

| Channel 类型 | 合并策略 | 典型用途 |
|---|---|---|
| `LastValue` | 后写覆盖前写 | 普通状态字段（计数器、当前步骤） |
| `BinaryOperatorAggregate` | 自定义 reducer 函数 | 消息列表（追加）、日志收集 |
| `EphemeralValue` | 不持久化，读后即弃 | 一次性传递的中间值 |
| `Topic` | 追加到队列，drain 清空 | 事件队列、待处理任务列表 |
| `NamedBarrierValue` | 追踪多源完成状态 | `add_edge_join` 内部使用 |

**为什么需要 Channel？** 因为并行节点可能同时写同一个 key。
比如 worker_a 和 worker_b 同时往 `results` 里写结果——
用 `LastValue` 就只保留最后一个，用 `BinaryOperatorAggregate` 就能追加成数组。

```rust
// 追加语义的 channel：多个节点写入会合并成数组
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

### Node —— "干活的单元"

每个节点实现 `GraphNode` trait，或者用 `FnNode` / `add_node_fn` 快速创建闭包节点：

```rust
graph.add_node_fn("my_node", |state, ctx| async move {
    // state: 所有通道的当前值
    // ctx:   提供 stream_tx（流式输出）、cancel（取消信号）、interrupt（中断）

    let mut out = HashMap::new();
    out.insert("result".into(), json!("done"));
    Ok(NodeOutput::Update(out))
});
```

节点返回 `NodeOutput`：
- `Update(values)` — 纯状态更新，引擎按边决定下一跳
- `Command { goto, update }` — 节点自己决定跳到哪（跳过边的路由逻辑）

### Edge —— "走哪条路"

```rust
// 静态边：A 完成后固定到 B
graph.add_edge("a", "b");

// 条件边：运行时根据状态动态决定
graph.add_conditional_edge("router", |state| {
    if state.get("done").and_then(|v| v.as_bool()) == Some(true) {
        RouteDecision::End           // 结束
    } else {
        RouteDecision::Next("worker".into())  // 继续
    }
});

// 条件边 + pathMap：语义 key 映射到实际节点名
graph.add_conditional_edge_with_map("classifier", router_fn, path_map);
```

### 扇出（Fan-out）—— "一变多并行"

**扇出是让一个节点的输出同时触发多个并行任务**，每个任务可以带独立的输入参数。

```
           ┌──→ worker("alpha") ──┐
dispatcher ┤                      ├──→ ...
           └──→ worker("beta")  ──┘
```

两种方式触发扇出：

```rust
// 方式 1：条件边返回 Send
graph.add_conditional_edge("dispatcher", |_state| {
    RouteDecision::Send(vec![
        Send::new("worker", json!({"item": "alpha"})),
        Send::new("worker", json!({"item": "beta"})),
    ])
});

// 方式 2：节点内通过 Command.goto 扇出
Ok(NodeOutput::command_with_sends(None, vec![
    Send::new("worker", json!({"id": 1})),
    Send::new("worker", json!({"id": 2})),
]))
```

每个 `Send` 产生一个 **PUSH 任务**——目标节点收到的 `state` 会混入 `Send.args`。
所有 PUSH 任务在同一个 superstep 内并行执行，结果通过 Channel 合并。

### 汇聚（Join）—— "多变一等待"

扇出的反面。等待多个节点全部完成后，才执行目标节点：

```rust
// A 和 B 都完成后才执行 merge
graph.add_edge_join(&["a", "b"], "merge");
```

内部通过 `NamedBarrierValue` 通道追踪完成状态。

## 执行模型：Superstep

引擎采用 **superstep** 模型循环执行：

```
Superstep 1: [task_A, task_B]  ← 并行执行
    ↓ 合并写入到 channels
    ↓ 保存 checkpoint
    ↓ 解析边 → 收集下一批 tasks
Superstep 2: [task_C]
    ↓ ...
完成（无更多 tasks）
```

同一 superstep 内的所有任务并行执行，结果批量合并。
这保证了确定性——无论并行任务的完成顺序如何，最终状态一致。

## 生命周期：构建 → 编译 → 执行

```rust
// 1. 构建（可变）
let mut graph = StateGraph::new(channels);
graph.add_node_fn("a", ...);
graph.add_edge(START, "a");
graph.add_edge("a", END);

// 2. 编译（不可变）—— 验证拓扑完整性
let compiled = graph.compile(CompileConfig {
    checkpointer: Some(saver),         // 检查点存储
    interrupt_before: vec!["tools".into()], // 工具调用前暂停
    retry_policy: Some(RetryPolicy::default()), // 失败自动重试
    ..Default::default()
})?;

// 3. 执行
let result = compiled.invoke(input, &GraphConfig {
    thread_id: Some("conversation-1".into()),
    step_timeout: Some(Duration::from_secs(30)),
    ..Default::default()
}).await?;
```

## 高级控制能力

### 中断与恢复（Human-in-the-loop）

三种中断方式：

```rust
// 1. 节点前中断（编译时配置）
CompileConfig { interrupt_before: vec!["dangerous_node".into()], .. }

// 2. 节点后中断（编译时配置）
CompileConfig { interrupt_after: vec!["review_node".into()], .. }

// 3. 节点内主动中断（运行时，最灵活）
graph.add_node_fn("planner", |mut state, ctx| async move {
    let plan = create_plan(&state);

    // 暂停执行，把 plan 展示给用户
    let feedback = ctx.interrupt(json!({"plan": plan, "question": "确认？"}))?;
    // ↑ 首次执行到这里会暂停
    // ↑ resume 后 feedback = 用户传入的值

    execute_plan(&plan, &feedback);
    Ok(NodeOutput::Update(state))
});

// 恢复执行
compiled.resume(&config, Some(Command {
    resume: Some(json!("approved")),  // 用户的回复
    update: None,
})).await?;
```

### 重试策略（RetryPolicy）

节点执行失败时自动重试，支持指数退避：

```rust
// 图级默认策略
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

// 节点级覆盖（实现 GraphNode trait 时）
fn retry_policy(&self) -> Option<&RetryPolicy> { Some(&self.custom_policy) }
```

仅 `AgentError::Llm` 和 `AgentError::Other` 视为可重试。

### 执行超时（StepTimeout）

```rust
GraphConfig {
    step_timeout: Some(Duration::from_secs(30)),
    ..Default::default()
}
// 节点级覆盖
fn timeout(&self) -> Option<Duration> { Some(Duration::from_secs(60)) }
```

### 子图（Subgraph）

将一个编译好的图作为另一个图的节点：

```rust
let inner = inner_graph.compile(CompileConfig::default())?;
outer_graph.add_subgraph("inner", inner);
```

子图有独立的 checkpoint namespace，内部事件通过 `SubgraphEvent` 冒泡到父图。

## 模块结构

```
eko-core/src/
├── channels/          # 状态通道系统
│   ├── base.rs        #   AnyChannel trait
│   ├── last_value.rs  #   覆盖写入
│   ├── binop.rs       #   自定义 reducer
│   ├── ephemeral.rs   #   临时值（不持久化）
│   ├── topic.rs       #   追加队列
│   └── barrier.rs     #   多源同步屏障
├── checkpoint/        # 检查点抽象
│   ├── saver.rs       #   BaseCheckpointSaver trait
│   ├── memory.rs      #   内存实现（测试用）
│   └── types.rs       #   Checkpoint / CheckpointTuple
├── core/              # 基础契约
│   ├── error.rs       #   AgentError / InterruptData
│   ├── llm.rs         #   LlmClient trait
│   ├── stream.rs      #   StreamEvent
│   ├── message.rs     #   Message / ToolCall
│   └── tool.rs        #   ToolDefinition trait
└── graph/             # 状态图引擎
    ├── builder.rs     #   StateGraph（构建期）
    ├── compiled.rs    #   CompiledGraph（执行期）
    ├── node.rs        #   GraphNode trait / FnNode / NodeContext
    ├── command.rs     #   NodeOutput / GotoTarget / CommandGraph
    ├── edge.rs        #   Edge / RouteDecision / START / END
    ├── send.rs        #   Send（扇出指令）
    ├── task.rs        #   Task（PULL / PUSH）
    ├── subgraph.rs    #   SubgraphNode
    └── types.rs       #   GraphResult / RetryPolicy / Command / ...
```

## 快速上手示例

### 最简单的线性图

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
// result = GraphResult::Done { values: {"msg": "hello world"}, .. }
```

### 条件循环

```rust
g.add_node_fn("increment", |mut s, _| async move {
    let v = s.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
    s.insert("count".into(), json!(v + 1));
    Ok(NodeOutput::Update(s))
});
g.add_edge(START, "increment");
g.add_conditional_edge("increment", |state| {
    let count = state.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
    if count >= 5 { RouteDecision::End } else { RouteDecision::Next("increment".into()) }
});
```

### 并行扇出 + 汇聚

```rust
g.add_node_fn("dispatcher", |_s, _| async move {
    Ok(NodeOutput::Update(HashMap::new()))
});
g.add_node_fn("worker_a", |_s, _| async move { /* ... */ });
g.add_node_fn("worker_b", |_s, _| async move { /* ... */ });
g.add_node_fn("merger",   |_s, _| async move { /* ... */ });

g.add_edge(START, "dispatcher");
g.add_conditional_edge("dispatcher", |_| RouteDecision::Send(vec![
    Send::new("worker_a", json!({})),
    Send::new("worker_b", json!({})),
]));
g.add_edge_join(&["worker_a", "worker_b"], "merger");
g.add_edge("merger", END);
```

### 便捷串联

```rust
g.add_sequence(vec![
    Box::new(FnNode::new("step1", |s, _| async move { /* ... */ })),
    Box::new(FnNode::new("step2", |s, _| async move { /* ... */ })),
    Box::new(FnNode::new("step3", |s, _| async move { /* ... */ })),
]);
g.add_edge(START, "step1");
g.add_edge("step3", END);
```

## 设计理念

```
eko-core（本库）            你的实现层                      你的应用
────────────────           ──────────                      ────────
StateGraph                 ReAct / Supervisor / Swarm      组装并运行
CompiledGraph              LlmClient impl (OpenAI 等)
AnyChannel                 CheckpointSaver impl (SQLite 等)
GraphNode trait            工具定义与执行
LlmClient trait            ToolExecutor
BaseCheckpointSaver trait  Session management
```

`eko-core` 定义契约（trait），你可以自行实现所有 trait 来对接自定义的 LLM / 存储 / 工具。
