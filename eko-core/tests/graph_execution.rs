use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use eko_core::channels::{BinaryOperatorAggregate, EphemeralValue, LastValue};
use eko_core::graph::command::NodeOutput;
use eko_core::graph::edge::{RouteDecision, END, START};
use eko_core::graph::node::FnNode;
use eko_core::graph::send::Send as GraphSend;
use eko_core::graph::types::InterruptReason;
use eko_core::core::stream::StreamMode;
use eko_core::graph::types::Durability;
use eko_core::{
    AgentError, AnyChannel, BaseCheckpointSaver, Command, CompileConfig, GraphConfig, GraphResult,
    MemorySaver, RetryPolicy, StateGraph,
};

fn last_value_channels(names: &[&str]) -> HashMap<String, Box<dyn AnyChannel>> {
    names
        .iter()
        .map(|n| (n.to_string(), Box::new(LastValue::new()) as Box<dyn AnyChannel>))
        .collect()
}

fn default_config() -> GraphConfig {
    GraphConfig {
        thread_id: Some("test-thread".into()),
        ..Default::default()
    }
}

// =========================================================================
// 线性图执行
// =========================================================================

#[tokio::test]
async fn linear_chain_a_b_end() {
    let mut g = StateGraph::new(last_value_channels(&["counter"]));

    g.add_node_fn("a", |mut state, _ctx| async move {
        let val = state.get("counter").and_then(|v| v.as_i64()).unwrap_or(0);
        state.insert("counter".into(), json!(val + 1));
        Ok(NodeOutput::Update(state))
    });
    g.add_node_fn("b", |mut state, _ctx| async move {
        let val = state.get("counter").and_then(|v| v.as_i64()).unwrap_or(0);
        state.insert("counter".into(), json!(val + 10));
        Ok(NodeOutput::Update(state))
    });

    g.add_edge(START, "a");
    g.add_edge("a", "b");
    g.add_edge("b", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let mut input = HashMap::new();
    input.insert("counter".into(), json!(0));

    let result = compiled.invoke(input, &default_config()).await.unwrap();
    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("counter"), Some(&json!(11)));
        }
        _ => panic!("Expected Done"),
    }
}

#[tokio::test]
async fn three_node_chain() {
    let mut g = StateGraph::new(last_value_channels(&["x"]));

    g.add_node_fn("step1", |mut s, _| async move {
        s.insert("x".into(), json!("step1"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("step2", |mut s, _| async move {
        let prev = s.get("x").and_then(|v| v.as_str()).unwrap_or("").to_string();
        s.insert("x".into(), json!(format!("{prev}->step2")));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("step3", |mut s, _| async move {
        let prev = s.get("x").and_then(|v| v.as_str()).unwrap_or("").to_string();
        s.insert("x".into(), json!(format!("{prev}->step3")));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "step1");
    g.add_edge("step1", "step2");
    g.add_edge("step2", "step3");
    g.add_edge("step3", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("x"), Some(&json!("step1->step2->step3")));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// 条件路由
// =========================================================================

#[tokio::test]
async fn conditional_routing_high() {
    let mut g = StateGraph::new(last_value_channels(&["score", "result"]));

    g.add_node_fn("classify", |s, _| async move { Ok(NodeOutput::Update(s)) });
    g.add_node_fn("high", |mut s, _| async move {
        s.insert("result".into(), json!("high_path"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("low", |mut s, _| async move {
        s.insert("result".into(), json!("low_path"));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "classify");
    g.add_conditional_edge("classify", |state| {
        let score = state
            .get("score")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if score >= 80 {
            RouteDecision::Next("high".into())
        } else {
            RouteDecision::Next("low".into())
        }
    });
    g.add_edge("high", END);
    g.add_edge("low", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();

    let mut input = HashMap::new();
    input.insert("score".into(), json!(90));
    let result = compiled.invoke(input, &default_config()).await.unwrap();
    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("result"), Some(&json!("high_path")));
        }
        _ => panic!("Expected Done"),
    }
}

#[tokio::test]
async fn conditional_routing_low() {
    let mut g = StateGraph::new(last_value_channels(&["score", "result"]));

    g.add_node_fn("classify", |s, _| async move { Ok(NodeOutput::Update(s)) });
    g.add_node_fn("high", |mut s, _| async move {
        s.insert("result".into(), json!("high_path"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("low", |mut s, _| async move {
        s.insert("result".into(), json!("low_path"));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "classify");
    g.add_conditional_edge("classify", |state| {
        let score = state
            .get("score")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if score >= 80 {
            RouteDecision::Next("high".into())
        } else {
            RouteDecision::Next("low".into())
        }
    });
    g.add_edge("high", END);
    g.add_edge("low", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();

    let mut input = HashMap::new();
    input.insert("score".into(), json!(50));
    let result = compiled.invoke(input, &default_config()).await.unwrap();
    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("result"), Some(&json!("low_path")));
        }
        _ => panic!("Expected Done"),
    }
}

#[tokio::test]
async fn conditional_edge_to_end() {
    let mut g = StateGraph::new(last_value_channels(&["done"]));

    g.add_node_fn("check", |s, _| async move { Ok(NodeOutput::Update(s)) });

    g.add_edge(START, "check");
    g.add_conditional_edge("check", |_| RouteDecision::End);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let mut input = HashMap::new();
    input.insert("done".into(), json!(true));
    let result = compiled.invoke(input, &default_config()).await.unwrap();
    assert!(matches!(result, GraphResult::Done { .. }));
}

#[tokio::test]
async fn conditional_start_routing() {
    let mut g = StateGraph::new(last_value_channels(&["mode", "result"]));

    g.add_node_fn("fast", |mut s, _| async move {
        s.insert("result".into(), json!("fast_mode"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("slow", |mut s, _| async move {
        s.insert("result".into(), json!("slow_mode"));
        Ok(NodeOutput::Update(s))
    });

    g.add_conditional_edge(START, |state| {
        let mode = state
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("slow");
        RouteDecision::Next(mode.to_string())
    });
    g.add_edge("fast", END);
    g.add_edge("slow", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();

    let mut input = HashMap::new();
    input.insert("mode".into(), json!("fast"));
    let result = compiled.invoke(input, &default_config()).await.unwrap();
    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("result"), Some(&json!("fast_mode")));
        }
        _ => panic!("Expected Done"),
    }
}

#[tokio::test]
async fn path_map_routing() {
    let mut g = StateGraph::new(last_value_channels(&["action", "output"]));

    g.add_node_fn("router", |s, _| async move { Ok(NodeOutput::Update(s)) });
    g.add_node_fn("create", |mut s, _| async move {
        s.insert("output".into(), json!("created"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("delete", |mut s, _| async move {
        s.insert("output".into(), json!("deleted"));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "router");

    let mut path_map = HashMap::new();
    path_map.insert("create".into(), "create".into());
    path_map.insert("delete".into(), "delete".into());
    path_map.insert("none".into(), END.into());

    g.add_conditional_edge_with_map(
        "router",
        |state| {
            state
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("none")
                .to_string()
        },
        path_map,
    );
    g.add_edge("create", END);
    g.add_edge("delete", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();

    let mut input = HashMap::new();
    input.insert("action".into(), json!("create"));
    let result = compiled.invoke(input, &default_config()).await.unwrap();
    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("output"), Some(&json!("created")));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// Command goto
// =========================================================================

#[tokio::test]
async fn command_goto_overrides_edge() {
    let mut g = StateGraph::new(last_value_channels(&["result"]));

    g.add_node_fn("start_node", |_s, _| async move {
        Ok(NodeOutput::command(
            Some({
                let mut m = HashMap::new();
                m.insert("result".into(), json!("via_goto"));
                m
            }),
            "target",
        ))
    });
    g.add_node_fn("wrong_target", |mut s, _| async move {
        s.insert("result".into(), json!("wrong"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("target", |s, _| async move {
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "start_node");
    g.add_edge("start_node", "wrong_target");
    g.add_edge("wrong_target", END);
    g.add_edge("target", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();
    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("result"), Some(&json!("via_goto")));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// BinaryOperatorAggregate 累加语义
// =========================================================================

#[tokio::test]
async fn binop_channel_accumulates_across_nodes() {
    let mut ch: HashMap<String, Box<dyn AnyChannel>> = HashMap::new();
    ch.insert(
        "log".into(),
        Box::new(BinaryOperatorAggregate::new(|old, new| {
            match old {
                Value::Array(mut arr) => {
                    arr.push(new);
                    Ok(Value::Array(arr))
                }
                _ => Ok(Value::Array(vec![old, new])),
            }
        })),
    );

    let mut g = StateGraph::new(ch);

    g.add_node_fn("a", |_s, _| async move {
        let mut out = HashMap::new();
        out.insert("log".into(), json!("from_a"));
        Ok(NodeOutput::Update(out))
    });
    g.add_node_fn("b", |_s, _| async move {
        let mut out = HashMap::new();
        out.insert("log".into(), json!("from_b"));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "a");
    g.add_edge("a", "b");
    g.add_edge("b", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("log"), Some(&json!(["from_a", "from_b"])));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// EphemeralValue 在节点间不持久
// =========================================================================

#[tokio::test]
async fn ephemeral_channel_not_persisted_to_checkpoint() {
    let mut ch: HashMap<String, Box<dyn AnyChannel>> = HashMap::new();
    ch.insert("persistent".into(), Box::new(LastValue::new()));
    ch.insert("ephemeral".into(), Box::new(EphemeralValue::new()));

    let saver = Arc::new(MemorySaver::new());

    let mut g = StateGraph::new(ch);
    g.add_node_fn("writer", |_s, _| async move {
        let mut out = HashMap::new();
        out.insert("persistent".into(), json!("kept"));
        out.insert("ephemeral".into(), json!("temporary"));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "writer");
    g.add_edge("writer", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            ..Default::default()
        })
        .unwrap();

    let config = default_config();
    let result = compiled.invoke(HashMap::new(), &config).await.unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("persistent"), Some(&json!("kept")));
        }
        _ => panic!("Expected Done"),
    }

    let state = compiled.get_state("test-thread").await.unwrap().unwrap();
    assert_eq!(state.values.get("persistent"), Some(&json!("kept")));
    assert!(state.values.get("ephemeral").is_none());
}

// =========================================================================
// Send 并行扇出
// =========================================================================

#[tokio::test]
async fn send_parallel_fan_out() {
    let mut ch: HashMap<String, Box<dyn AnyChannel>> = HashMap::new();
    ch.insert(
        "results".into(),
        Box::new(BinaryOperatorAggregate::new(|old, new| {
            match old {
                Value::Array(mut arr) => {
                    arr.push(new);
                    Ok(Value::Array(arr))
                }
                _ => Ok(Value::Array(vec![old, new])),
            }
        })),
    );

    let mut g = StateGraph::new(ch);

    g.add_node_fn("dispatcher", |_s, _| async move {
        Ok(NodeOutput::Update(HashMap::new()))
    });
    g.add_node_fn("worker", |state, _| async move {
        let item = state
            .get("item")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let mut out = HashMap::new();
        out.insert("results".into(), json!(format!("processed:{item}")));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "dispatcher");
    g.add_conditional_edge("dispatcher", |_state| {
        RouteDecision::Send(vec![
            GraphSend::new("worker", json!({"item": "alpha"})),
            GraphSend::new("worker", json!({"item": "beta"})),
        ])
    });
    g.add_edge("worker", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            let results = values.get("results").expect("results should exist");
            let arr = results.as_array().expect("should be array");
            assert_eq!(arr.len(), 2);
            let strings: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            assert!(strings.contains(&"processed:alpha"));
            assert!(strings.contains(&"processed:beta"));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// 中断 (interrupt_before / interrupt_after)
// =========================================================================

#[tokio::test]
async fn interrupt_before_node() {
    let saver = Arc::new(MemorySaver::new());
    let mut g = StateGraph::new(last_value_channels(&["data"]));

    g.add_node_fn("safe", |mut s, _| async move {
        s.insert("data".into(), json!("safe_done"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("dangerous", |mut s, _| async move {
        s.insert("data".into(), json!("danger_done"));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "safe");
    g.add_edge("safe", "dangerous");
    g.add_edge("dangerous", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            interrupt_before: vec!["dangerous".into()],
            ..Default::default()
        })
        .unwrap();

    let config = default_config();
    let result = compiled
        .invoke(HashMap::new(), &config)
        .await
        .unwrap();

    match &result {
        GraphResult::Interrupted {
            resume_node,
            reason,
            values,
            ..
        } => {
            assert_eq!(resume_node, "dangerous");
            assert!(matches!(reason, InterruptReason::InterruptBefore(n) if n == "dangerous"));
            assert_eq!(values.get("data"), Some(&json!("safe_done")));
        }
        _ => panic!("Expected Interrupted, got {:?}", result),
    }
}

#[tokio::test]
async fn interrupt_after_node() {
    let saver = Arc::new(MemorySaver::new());
    let mut g = StateGraph::new(last_value_channels(&["data"]));

    g.add_node_fn("monitored", |mut s, _| async move {
        s.insert("data".into(), json!("monitored_done"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("next", |mut s, _| async move {
        s.insert("data".into(), json!("next_done"));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "monitored");
    g.add_edge("monitored", "next");
    g.add_edge("next", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            interrupt_after: vec!["monitored".into()],
            ..Default::default()
        })
        .unwrap();

    let config = default_config();
    let result = compiled
        .invoke(HashMap::new(), &config)
        .await
        .unwrap();

    match &result {
        GraphResult::Interrupted {
            resume_node,
            reason,
            values,
            ..
        } => {
            assert_eq!(resume_node, "monitored");
            assert!(matches!(reason, InterruptReason::InterruptAfter(n) if n == "monitored"));
            assert_eq!(values.get("data"), Some(&json!("monitored_done")));
        }
        _ => panic!("Expected Interrupted, got {:?}", result),
    }
}

// =========================================================================
// Resume 从中断恢复
// =========================================================================

#[tokio::test]
async fn resume_after_interrupt_before() {
    let saver = Arc::new(MemorySaver::new());
    let mut g = StateGraph::new(last_value_channels(&["data"]));

    g.add_node_fn("safe", |mut s, _| async move {
        s.insert("data".into(), json!("safe_done"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("review", |mut s, _| async move {
        let prev = s
            .get("data")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        s.insert("data".into(), json!(format!("{prev}+reviewed")));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "safe");
    g.add_edge("safe", "review");
    g.add_edge("review", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            interrupt_before: vec!["review".into()],
            ..Default::default()
        })
        .unwrap();

    let config = default_config();
    let result = compiled
        .invoke(HashMap::new(), &config)
        .await
        .unwrap();
    assert!(matches!(result, GraphResult::Interrupted { .. }));

    let resumed = compiled.resume(&config, None).await.unwrap();
    match resumed {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("data"), Some(&json!("safe_done+reviewed")));
        }
        _ => panic!("Expected Done after resume"),
    }
}

// =========================================================================
// Checkpoint 持久化 + get_state / get_state_history
// =========================================================================

#[tokio::test]
async fn checkpoint_saves_and_restores() {
    let saver = Arc::new(MemorySaver::new());
    let mut g = StateGraph::new(last_value_channels(&["counter"]));

    g.add_node_fn("inc", |mut s, _| async move {
        let v = s.get("counter").and_then(|v| v.as_i64()).unwrap_or(0);
        s.insert("counter".into(), json!(v + 1));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "inc");
    g.add_edge("inc", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            ..Default::default()
        })
        .unwrap();

    let config = default_config();
    let mut input = HashMap::new();
    input.insert("counter".into(), json!(0));
    compiled.invoke(input, &config).await.unwrap();

    let state = compiled.get_state("test-thread").await.unwrap().unwrap();
    assert_eq!(state.values.get("counter"), Some(&json!(1)));
}

#[tokio::test]
async fn get_state_history_has_multiple_entries() {
    let saver = Arc::new(MemorySaver::new());
    let mut g = StateGraph::new(last_value_channels(&["step"]));

    g.add_node_fn("a", |mut s, _| async move {
        s.insert("step".into(), json!("a"));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("b", |mut s, _| async move {
        s.insert("step".into(), json!("b"));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "a");
    g.add_edge("a", "b");
    g.add_edge("b", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            ..Default::default()
        })
        .unwrap();

    let config = default_config();
    compiled.invoke(HashMap::new(), &config).await.unwrap();

    let history = compiled.get_state_history("test-thread").await.unwrap();
    assert!(
        history.len() >= 3,
        "Should have input + loop checkpoints, got {}",
        history.len()
    );
}

// =========================================================================
// update_state 外部注入
// =========================================================================

#[tokio::test]
async fn update_state_injects_values() {
    let saver = Arc::new(MemorySaver::new());
    let mut g = StateGraph::new(last_value_channels(&["key"]));

    g.add_node_fn("noop", |s, _| async move { Ok(NodeOutput::Update(s)) });
    g.add_edge(START, "noop");
    g.add_edge("noop", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            ..Default::default()
        })
        .unwrap();

    let config = default_config();
    compiled.invoke(HashMap::new(), &config).await.unwrap();

    let mut updates = HashMap::new();
    updates.insert("key".into(), json!("injected"));
    compiled
        .update_state("test-thread", updates, Some("noop"))
        .await
        .unwrap();

    let state = compiled.get_state("test-thread").await.unwrap().unwrap();
    assert_eq!(state.values.get("key"), Some(&json!("injected")));
}

// =========================================================================
// 用户取消
// =========================================================================

#[tokio::test]
async fn cancel_interrupts_execution() {
    let mut g = StateGraph::new(last_value_channels(&["data"]));

    g.add_node_fn("slow", |mut s, _| async move {
        s.insert("data".into(), json!("done"));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "slow");
    g.add_edge("slow", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(true);
    drop(cancel_tx);

    let config = GraphConfig {
        thread_id: Some("cancel-test".into()),
        cancel: Some(cancel_rx),
        ..Default::default()
    };

    let result = compiled.invoke(HashMap::new(), &config).await.unwrap();
    assert!(matches!(result, GraphResult::Interrupted { reason: InterruptReason::UserCancelled, .. }));
}

// =========================================================================
// 子图执行
// =========================================================================

#[tokio::test]
async fn subgraph_execution() {
    let mut inner = StateGraph::new(last_value_channels(&["val"]));
    inner.add_node_fn("double", |mut s, _| async move {
        let v = s.get("val").and_then(|v| v.as_i64()).unwrap_or(0);
        s.insert("val".into(), json!(v * 2));
        Ok(NodeOutput::Update(s))
    });
    inner.add_edge(START, "double");
    inner.add_edge("double", END);
    let inner_compiled = inner.compile(CompileConfig::default()).unwrap();

    let mut outer = StateGraph::new(last_value_channels(&["val"]));
    outer.add_node_fn("setup", |mut s, _| async move {
        s.insert("val".into(), json!(5));
        Ok(NodeOutput::Update(s))
    });
    outer.add_subgraph("inner_graph", inner_compiled);

    outer.add_edge(START, "setup");
    outer.add_edge("setup", "inner_graph");
    outer.add_edge("inner_graph", END);

    let compiled = outer.compile(CompileConfig::default()).unwrap();
    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("val"), Some(&json!(10)));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// 循环图（条件退出）
// =========================================================================

#[tokio::test]
async fn loop_with_conditional_exit() {
    let mut g = StateGraph::new(last_value_channels(&["count"]));

    g.add_node_fn("increment", |mut s, _| async move {
        let v = s.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
        s.insert("count".into(), json!(v + 1));
        Ok(NodeOutput::Update(s))
    });

    g.add_edge(START, "increment");
    g.add_conditional_edge("increment", |state| {
        let count = state
            .get("count")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if count >= 5 {
            RouteDecision::End
        } else {
            RouteDecision::Next("increment".into())
        }
    });

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let mut input = HashMap::new();
    input.insert("count".into(), json!(0));
    let result = compiled.invoke(input, &default_config()).await.unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("count"), Some(&json!(5)));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// 无 checkpointer 也能运行
// =========================================================================

#[tokio::test]
async fn runs_without_checkpointer() {
    let mut g = StateGraph::new(last_value_channels(&["out"]));

    g.add_node_fn("node", |mut s, _| async move {
        s.insert("out".into(), json!("ok"));
        Ok(NodeOutput::Update(s))
    });
    g.add_edge(START, "node");
    g.add_edge("node", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let config = GraphConfig::default();
    let result = compiled.invoke(HashMap::new(), &config).await.unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("out"), Some(&json!("ok")));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// StreamEvent 发送
// =========================================================================

#[tokio::test]
async fn stream_events_are_sent() {
    let mut g = StateGraph::new(last_value_channels(&["x"]));

    g.add_node_fn("emitter", |s, ctx| async move {
        let _ = ctx.stream_tx.send(eko_core::StreamEvent::text("hello from node"));
        Ok(NodeOutput::Update(s))
    });
    g.add_edge(START, "emitter");
    g.add_edge("emitter", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let config = GraphConfig {
        thread_id: Some("stream-test".into()),
        stream_tx: Some(tx),
        ..Default::default()
    };

    compiled.invoke(HashMap::new(), &config).await.unwrap();

    let mut found_text = false;
    while let Ok(event) = rx.try_recv() {
        if let eko_core::StreamEvent::Message { content, .. } = event {
            if content == "hello from node" {
                found_text = true;
            }
        }
    }
    assert!(found_text, "Should have received the text stream event");
}

// =========================================================================
// RetryPolicy
// =========================================================================

#[tokio::test]
async fn retry_policy_retries_until_success() {
    let attempt_count = Arc::new(AtomicU32::new(0));
    let count = attempt_count.clone();

    let mut g = StateGraph::new(last_value_channels(&["result"]));
    g.add_node_fn("flaky", move |mut s, _| {
        let c = count.clone();
        async move {
            let n = c.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                return Err(AgentError::Llm("temporary failure".into()));
            }
            s.insert("result".into(), json!("success"));
            Ok(NodeOutput::Update(s))
        }
    });
    g.add_edge(START, "flaky");
    g.add_edge("flaky", END);

    let compiled = g
        .compile(CompileConfig {
            retry_policy: Some(RetryPolicy {
                max_attempts: 3,
                initial_interval_ms: 10,
                backoff_factor: 1.0,
                max_interval_ms: 100,
                jitter: false,
            }),
            ..Default::default()
        })
        .unwrap();

    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();
    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("result"), Some(&json!("success")));
        }
        _ => panic!("Expected Done"),
    }
    assert_eq!(attempt_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retry_policy_exhausted_propagates_error() {
    let mut g = StateGraph::new(last_value_channels(&["x"]));
    g.add_node_fn("always_fail", |_s, _| async move {
        Err(AgentError::Llm("permanent failure".into()))
    });
    g.add_edge(START, "always_fail");
    g.add_edge("always_fail", END);

    let compiled = g
        .compile(CompileConfig {
            retry_policy: Some(RetryPolicy {
                max_attempts: 2,
                initial_interval_ms: 10,
                backoff_factor: 1.0,
                max_interval_ms: 100,
                jitter: false,
            }),
            ..Default::default()
        })
        .unwrap();

    let result = compiled.invoke(HashMap::new(), &default_config()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("permanent failure"));
}

#[tokio::test]
async fn retry_policy_non_retryable_error_not_retried() {
    let attempt_count = Arc::new(AtomicU32::new(0));
    let count = attempt_count.clone();

    let mut g = StateGraph::new(last_value_channels(&["x"]));
    g.add_node_fn("config_err", move |_s, _| {
        let c = count.clone();
        async move {
            c.fetch_add(1, Ordering::SeqCst);
            Err(AgentError::Config("bad config".into()))
        }
    });
    g.add_edge(START, "config_err");
    g.add_edge("config_err", END);

    let compiled = g
        .compile(CompileConfig {
            retry_policy: Some(RetryPolicy {
                max_attempts: 5,
                initial_interval_ms: 10,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();

    let result = compiled.invoke(HashMap::new(), &default_config()).await;
    assert!(result.is_err());
    assert_eq!(attempt_count.load(Ordering::SeqCst), 1);
}

// =========================================================================
// StepTimeout
// =========================================================================

#[tokio::test]
async fn step_timeout_kills_slow_node() {
    let mut g = StateGraph::new(last_value_channels(&["x"]));
    g.add_node_fn("slow", |_s, _| async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        Ok(NodeOutput::Update(HashMap::new()))
    });
    g.add_edge(START, "slow");
    g.add_edge("slow", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let config = GraphConfig {
        thread_id: Some("timeout-test".into()),
        step_timeout: Some(Duration::from_millis(50)),
        ..Default::default()
    };

    let result = compiled.invoke(HashMap::new(), &config).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, eko_core::AgentError::NodeTimeout { .. }));
}

#[tokio::test]
async fn step_timeout_fast_node_succeeds() {
    let mut g = StateGraph::new(last_value_channels(&["result"]));
    g.add_node_fn("fast", |mut s, _| async move {
        s.insert("result".into(), json!("quick"));
        Ok(NodeOutput::Update(s))
    });
    g.add_edge(START, "fast");
    g.add_edge("fast", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let config = GraphConfig {
        thread_id: Some("timeout-ok".into()),
        step_timeout: Some(Duration::from_secs(5)),
        ..Default::default()
    };

    let result = compiled.invoke(HashMap::new(), &config).await.unwrap();
    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("result"), Some(&json!("quick")));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// 节点内 interrupt()
// =========================================================================

#[tokio::test]
async fn node_interrupt_single_pause_and_resume() {
    let saver = Arc::new(MemorySaver::new());
    let mut g = StateGraph::new(last_value_channels(&["data"]));

    g.add_node_fn("review", |mut s, ctx| async move {
        let feedback = ctx.interrupt(json!({"question": "approve?"}))?;
        let msg = format!("approved:{}", feedback.as_str().unwrap_or("?"));
        s.insert("data".into(), json!(msg));
        Ok(NodeOutput::Update(s))
    });
    g.add_edge(START, "review");
    g.add_edge("review", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            ..Default::default()
        })
        .unwrap();

    let config = default_config();
    let result = compiled
        .invoke(HashMap::new(), &config)
        .await
        .unwrap();

    match &result {
        GraphResult::Interrupted {
            reason,
            resume_node,
            ..
        } => {
            assert_eq!(resume_node, "review");
            match reason {
                InterruptReason::NodeInterrupt { value } => {
                    assert_eq!(value, &json!({"question": "approve?"}));
                }
                _ => panic!("Expected NodeInterrupt reason"),
            }
        }
        _ => panic!("Expected Interrupted, got {:?}", result),
    }

    let resumed = compiled
        .resume(
            &config,
            Some(Command {
                resume: Some(json!("yes")),
                update: None,
            }),
        )
        .await
        .unwrap();

    match resumed {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("data"), Some(&json!("approved:yes")));
        }
        _ => panic!("Expected Done after resume"),
    }
}

#[tokio::test]
async fn node_interrupt_multiple_pauses_and_resumes() {
    let saver = Arc::new(MemorySaver::new());
    let mut g = StateGraph::new(last_value_channels(&["data"]));

    g.add_node_fn("multi_step", |mut s, ctx| async move {
        let step1 = ctx.interrupt(json!({"step": 1}))?;
        let step2 = ctx.interrupt(json!({"step": 2}))?;
        let msg = format!(
            "{}+{}",
            step1.as_str().unwrap_or("?"),
            step2.as_str().unwrap_or("?")
        );
        s.insert("data".into(), json!(msg));
        Ok(NodeOutput::Update(s))
    });
    g.add_edge(START, "multi_step");
    g.add_edge("multi_step", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            ..Default::default()
        })
        .unwrap();

    let config = default_config();

    let r1 = compiled
        .invoke(HashMap::new(), &config)
        .await
        .unwrap();
    assert!(matches!(r1, GraphResult::Interrupted { .. }));

    let r2 = compiled
        .resume(
            &config,
            Some(Command {
                resume: Some(json!("a")),
                update: None,
            }),
        )
        .await
        .unwrap();
    assert!(matches!(r2, GraphResult::Interrupted { .. }));

    let r3 = compiled
        .resume(
            &config,
            Some(Command {
                resume: Some(json!("b")),
                update: None,
            }),
        )
        .await
        .unwrap();

    match r3 {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("data"), Some(&json!("a+b")));
        }
        _ => panic!("Expected Done after second resume"),
    }
}

// =========================================================================
// Barrier/Join
// =========================================================================

#[tokio::test]
async fn barrier_join_waits_for_all_sources() {
    let mut ch: HashMap<String, Box<dyn AnyChannel>> = HashMap::new();
    ch.insert(
        "results".into(),
        Box::new(BinaryOperatorAggregate::new(|old, new| match old {
            Value::Array(mut arr) => {
                arr.push(new);
                Ok(Value::Array(arr))
            }
            _ => Ok(Value::Array(vec![old, new])),
        })),
    );
    ch.insert("final".into(), Box::new(LastValue::new()));

    let mut g = StateGraph::new(ch);

    g.add_node_fn("dispatcher", |_s, _| async move {
        Ok(NodeOutput::Update(HashMap::new()))
    });
    g.add_node_fn("a", |_s, _| async move {
        let mut out = HashMap::new();
        out.insert("results".into(), json!("a_done"));
        Ok(NodeOutput::Update(out))
    });
    g.add_node_fn("b", |_s, _| async move {
        let mut out = HashMap::new();
        out.insert("results".into(), json!("b_done"));
        Ok(NodeOutput::Update(out))
    });
    g.add_node_fn("merge", |_s, _| async move {
        let mut out = HashMap::new();
        out.insert("final".into(), json!("merged"));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "dispatcher");
    g.add_conditional_edge("dispatcher", |_| {
        RouteDecision::Send(vec![
            GraphSend::new("a", json!({})),
            GraphSend::new("b", json!({})),
        ])
    });
    g.add_edge_join(&["a", "b"], "merge");
    g.add_edge("merge", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("final"), Some(&json!("merged")));
            let results = values.get("results").unwrap().as_array().unwrap();
            assert_eq!(results.len(), 2);
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// addSequence
// =========================================================================

#[tokio::test]
async fn add_sequence_chains_nodes_automatically() {
    let mut g = StateGraph::new(last_value_channels(&["x"]));

    g.add_sequence(vec![
        Box::new(FnNode::new("s1", |mut s, _| async move {
            s.insert("x".into(), json!("s1"));
            Ok(NodeOutput::Update(s))
        })),
        Box::new(FnNode::new("s2", |mut s, _| async move {
            let prev = s
                .get("x")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            s.insert("x".into(), json!(format!("{prev}->s2")));
            Ok(NodeOutput::Update(s))
        })),
        Box::new(FnNode::new("s3", |mut s, _| async move {
            let prev = s
                .get("x")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            s.insert("x".into(), json!(format!("{prev}->s3")));
            Ok(NodeOutput::Update(s))
        })),
    ]);

    g.add_edge(START, "s1");
    g.add_edge("s3", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("x"), Some(&json!("s1->s2->s3")));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// Command.goto Send 数组（扇出）
// =========================================================================

#[tokio::test]
async fn command_goto_sends_fan_out() {
    let mut ch: HashMap<String, Box<dyn AnyChannel>> = HashMap::new();
    ch.insert(
        "results".into(),
        Box::new(BinaryOperatorAggregate::new(|old, new| match old {
            Value::Array(mut arr) => {
                arr.push(new);
                Ok(Value::Array(arr))
            }
            _ => Ok(Value::Array(vec![old, new])),
        })),
    );

    let mut g = StateGraph::new(ch);

    g.add_node_fn("router", |_s, _| async move {
        Ok(NodeOutput::command_with_sends(
            None,
            vec![
                GraphSend::new("worker", json!({"id": 1})),
                GraphSend::new("worker", json!({"id": 2})),
            ],
        ))
    });
    g.add_node_fn("worker", |state, _| async move {
        let id = state.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let mut out = HashMap::new();
        out.insert("results".into(), json!(format!("done:{id}")));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "router");
    g.add_edge("router", END);
    g.add_edge("worker", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            let results = values.get("results").expect("results should exist");
            let arr = results.as_array().expect("should be array");
            assert_eq!(arr.len(), 2);
            let strings: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            assert!(strings.contains(&"done:1"));
            assert!(strings.contains(&"done:2"));
        }
        _ => panic!("Expected Done"),
    }
}

// =========================================================================
// add_edge_join: 不等长分支
// =========================================================================

#[tokio::test]
async fn barrier_join_uneven_branches() {
    // 拓扑：
    //   START → a ──(Send)──→ b → b2 ─┐
    //                    └──→ c ───────┼→ d (join) → END
    //
    // b 分支长度 2（b → b2），c 分支长度 1。
    // d 必须等 b2 和 c 都完成后才执行。
    let mut ch: HashMap<String, Box<dyn AnyChannel>> = HashMap::new();
    ch.insert(
        "results".into(),
        Box::new(BinaryOperatorAggregate::new(|old, new| match old {
            Value::Array(mut arr) => {
                arr.push(new);
                Ok(Value::Array(arr))
            }
            _ => Ok(Value::Array(vec![old, new])),
        })),
    );
    ch.insert("final".into(), Box::new(LastValue::new()));

    let mut g = StateGraph::new(ch);

    g.add_node_fn("a", |_s, _| async move {
        Ok(NodeOutput::Update(HashMap::new()))
    });
    g.add_node_fn("b", |_s, _| async move {
        let mut out = HashMap::new();
        out.insert("results".into(), json!("b_done"));
        Ok(NodeOutput::Update(out))
    });
    g.add_node_fn("b2", |_s, _| async move {
        let mut out = HashMap::new();
        out.insert("results".into(), json!("b2_done"));
        Ok(NodeOutput::Update(out))
    });
    g.add_node_fn("c", |_s, _| async move {
        let mut out = HashMap::new();
        out.insert("results".into(), json!("c_done"));
        Ok(NodeOutput::Update(out))
    });
    g.add_node_fn("d", |s, _| async move {
        let results = s
            .get("results")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let mut out = HashMap::new();
        out.insert("final".into(), json!(format!("merged_{results}_items")));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "a");
    g.add_conditional_edge("a", |_| {
        RouteDecision::Send(vec![
            GraphSend::new("b", json!({})),
            GraphSend::new("c", json!({})),
        ])
    });
    g.add_edge("b", "b2");
    g.add_edge_join(&["b2", "c"], "d");
    g.add_edge("d", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let result = compiled
        .invoke(HashMap::new(), &default_config())
        .await
        .unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            let final_val = values.get("final").expect("final should exist");
            let results = values.get("results").and_then(|v| v.as_array());
            assert!(results.is_some(), "results should be an array");
            let arr = results.unwrap();
            let strings: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
            assert!(
                strings.contains(&"b_done"),
                "should contain b_done, got: {strings:?}",
            );
            assert!(
                strings.contains(&"b2_done"),
                "should contain b2_done, got: {strings:?}",
            );
            assert!(
                strings.contains(&"c_done"),
                "should contain c_done, got: {strings:?}",
            );
            assert_eq!(final_val, &json!("merged_3_items"));
        }
        other => panic!("Expected Done, got: {other:?}"),
    }
}

// =========================================================================
// 并发控制 (max_concurrency)
// =========================================================================

#[tokio::test]
async fn max_concurrency_limits_parallel_tasks() {
    use std::sync::atomic::AtomicUsize;

    let peak = Arc::new(AtomicUsize::new(0));
    let current = Arc::new(AtomicUsize::new(0));

    let mut channels: HashMap<String, Box<dyn AnyChannel>> = HashMap::new();
    channels.insert(
        "log".into(),
        Box::new(BinaryOperatorAggregate::new(|a: Value, b: Value| {
            let mut arr = a.as_array().cloned().unwrap_or_default();
            arr.push(b);
            Ok(Value::Array(arr))
        })),
    );

    let mut g = StateGraph::new(channels);

    for name in &["a", "b", "c", "d"] {
        let p = peak.clone();
        let c = current.clone();
        let n = name.to_string();
        g.add_node(FnNode::new(n.clone(), move |mut state, _| {
            let p = p.clone();
            let c = c.clone();
            let n = n.clone();
            async move {
                let prev = c.fetch_add(1, Ordering::SeqCst);
                p.fetch_max(prev + 1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(30)).await;
                c.fetch_sub(1, Ordering::SeqCst);
                state.insert("log".into(), json!(n));
                Ok(NodeOutput::Update(state))
            }
        }));
    }

    g.add_edge(START, "a");
    g.add_conditional_edge("a", |_| RouteDecision::Send(vec![
        GraphSend::new("b", json!({})),
        GraphSend::new("c", json!({})),
        GraphSend::new("d", json!({})),
    ]));
    g.add_edge("b", END);
    g.add_edge("c", END);
    g.add_edge("d", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let config = GraphConfig {
        thread_id: Some("concurrency-test".into()),
        max_concurrency: Some(2),
        ..Default::default()
    };
    let result = compiled.invoke(HashMap::new(), &config).await.unwrap();
    assert!(matches!(result, GraphResult::Done { .. }));
    assert!(peak.load(Ordering::SeqCst) <= 2, "peak concurrency should be <= 2");
}

// =========================================================================
// Durability 策略
// =========================================================================

#[tokio::test]
async fn durability_exit_saves_only_at_end() {
    let saver = Arc::new(MemorySaver::new());
    let mut g = StateGraph::new(last_value_channels(&["v"]));
    g.add_node_fn("a", |mut s, _| async move {
        s.insert("v".into(), json!(1));
        Ok(NodeOutput::Update(s))
    });
    g.add_node_fn("b", |mut s, _| async move {
        s.insert("v".into(), json!(2));
        Ok(NodeOutput::Update(s))
    });
    g.add_edge(START, "a");
    g.add_edge("a", "b");
    g.add_edge("b", END);

    let compiled = g
        .compile(CompileConfig {
            checkpointer: Some(saver.clone()),
            ..Default::default()
        })
        .unwrap();

    let config = GraphConfig {
        thread_id: Some("dur-exit".into()),
        durability: Durability::Exit,
        ..Default::default()
    };

    let result = compiled.invoke(HashMap::new(), &config).await.unwrap();
    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("v"), Some(&json!(2)));
        }
        other => panic!("Expected Done, got: {other:?}"),
    }

    let history = saver.list("dur-exit", None).await.unwrap();
    assert!(
        !history.is_empty(),
        "should have at least one checkpoint at exit"
    );
}

// =========================================================================
// StreamMode 过滤
// =========================================================================

#[tokio::test]
async fn stream_mode_filters_events() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let mut g = StateGraph::new(last_value_channels(&["v"]));
    g.add_node_fn("a", |mut s, ctx| async move {
        ctx.emit("my_event", json!({"key": "value"}));
        s.insert("v".into(), json!(42));
        Ok(NodeOutput::Update(s))
    });
    g.add_edge(START, "a");
    g.add_edge("a", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();

    // Only enable Custom mode, not Values/Updates/Debug
    let config = GraphConfig {
        thread_id: Some("stream-filter-test".into()),
        stream_tx: Some(tx),
        stream_modes: Some(std::collections::HashSet::from([StreamMode::Custom])),
        ..Default::default()
    };

    let _ = compiled.invoke(HashMap::new(), &config).await.unwrap();

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    // Custom events should pass through
    let custom_count = events
        .iter()
        .filter(|e| matches!(e, eko_core::StreamEvent::Custom { .. }))
        .count();
    assert_eq!(custom_count, 1);

    // StateValues events should be filtered out
    let state_values_count = events
        .iter()
        .filter(|e| matches!(e, eko_core::StreamEvent::StateValues { .. }))
        .count();
    assert_eq!(state_values_count, 0);
}

// =========================================================================
// Error type: RecursionLimit
// =========================================================================

#[tokio::test]
async fn recursion_limit_returns_typed_error() {
    let mut g = StateGraph::new(last_value_channels(&["counter"]));
    g.add_node_fn("loop_node", |mut state, _| async move {
        let v = state
            .get("counter")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        state.insert("counter".into(), json!(v + 1));
        Ok(NodeOutput::Update(state))
    });
    g.add_edge(START, "loop_node");
    g.add_edge("loop_node", "loop_node");

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let result = compiled.invoke(HashMap::new(), &default_config()).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, AgentError::RecursionLimit { .. }),
        "expected RecursionLimit, got: {err:?}"
    );
}

// =========================================================================
// NodeContext.emit() 自定义事件
// =========================================================================

#[tokio::test]
async fn node_emit_sends_custom_event() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let mut g = StateGraph::new(last_value_channels(&["v"]));
    g.add_node_fn("emitter", |mut s, ctx| async move {
        ctx.emit("progress", json!({"pct": 50}));
        ctx.emit("progress", json!({"pct": 100}));
        s.insert("v".into(), json!("done"));
        Ok(NodeOutput::Update(s))
    });
    g.add_edge(START, "emitter");
    g.add_edge("emitter", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();
    let config = GraphConfig {
        thread_id: Some("emit-test".into()),
        stream_tx: Some(tx),
        stream_modes: Some(StreamMode::all()),
        ..Default::default()
    };
    let _ = compiled.invoke(HashMap::new(), &config).await.unwrap();

    let mut custom_events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let eko_core::StreamEvent::Custom { name, data } = ev {
            custom_events.push((name, data));
        }
    }
    assert_eq!(custom_events.len(), 2);
    assert_eq!(custom_events[0].0, "progress");
    assert_eq!(custom_events[1].1, json!({"pct": 100}));
}
