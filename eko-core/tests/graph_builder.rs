use std::collections::HashMap;

use eko_core::channels::{LastValue, BinaryOperatorAggregate};
use eko_core::graph::command::NodeOutput;
use eko_core::graph::edge::{RouteDecision, START, END};
use eko_core::{CompileConfig, StateGraph};

fn make_channels() -> HashMap<String, Box<dyn eko_core::AnyChannel>> {
    let mut ch: HashMap<String, Box<dyn eko_core::AnyChannel>> = HashMap::new();
    ch.insert("value".into(), Box::new(LastValue::new()));
    ch
}

fn noop_node(name: &str) -> eko_core::FnNode {
    let n = name.to_string();
    eko_core::FnNode::new(n, |state, _ctx| async move {
        Ok(NodeOutput::Update(state))
    })
}

// =========================================================================
// 成功编译场景
// =========================================================================

#[test]
fn compile_simple_linear_graph() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("a"));
    g.add_node(noop_node("b"));
    g.add_edge(START, "a");
    g.add_edge("a", "b");
    g.add_edge("b", END);
    let result = g.compile(CompileConfig::default());
    assert!(result.is_ok());
}

#[test]
fn compile_single_node() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("only"));
    g.add_edge(START, "only");
    g.add_edge("only", END);
    assert!(g.compile(CompileConfig::default()).is_ok());
}

#[test]
fn compile_with_conditional_edge() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("a"));
    g.add_node(noop_node("b"));
    g.add_edge(START, "a");
    g.add_conditional_edge("a", |_state| RouteDecision::Next("b".into()));
    g.add_edge("b", END);
    assert!(g.compile(CompileConfig::default()).is_ok());
}

#[test]
fn compile_with_conditional_start() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("a"));
    g.add_node(noop_node("b"));
    g.add_conditional_edge(START, |_state| RouteDecision::Next("a".into()));
    g.add_edge("a", "b");
    g.add_edge("b", END);
    assert!(g.compile(CompileConfig::default()).is_ok());
}

#[test]
fn compile_with_path_map() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("hello"));
    g.add_node(noop_node("goodbye"));
    g.add_edge(START, "hello");

    let mut path_map = HashMap::new();
    path_map.insert("next".into(), "goodbye".into());
    path_map.insert("end".into(), END.into());

    g.add_conditional_edge_with_map("hello", |_state| "next".into(), path_map);
    g.add_edge("goodbye", END);
    assert!(g.compile(CompileConfig::default()).is_ok());
}

#[test]
fn compile_with_fn_node() {
    let mut g = StateGraph::new(make_channels());
    g.add_node_fn("my_fn", |state, _ctx| async move {
        Ok(NodeOutput::Update(state))
    });
    g.add_edge(START, "my_fn");
    g.add_edge("my_fn", END);
    assert!(g.compile(CompileConfig::default()).is_ok());
}

#[test]
fn compile_with_binop_channel() {
    let mut ch: HashMap<String, Box<dyn eko_core::AnyChannel>> = HashMap::new();
    ch.insert(
        "log".into(),
        Box::new(BinaryOperatorAggregate::new(|old, new| {
            match old {
                serde_json::Value::Array(mut arr) => {
                    arr.push(new);
                    Ok(serde_json::Value::Array(arr))
                }
                _ => Ok(serde_json::Value::Array(vec![old, new])),
            }
        })),
    );
    let mut g = StateGraph::new(ch);
    g.add_node(noop_node("node"));
    g.add_edge(START, "node");
    g.add_edge("node", END);
    assert!(g.compile(CompileConfig::default()).is_ok());
}

// =========================================================================
// 失败编译场景
// =========================================================================

#[test]
fn compile_fail_no_entry() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("a"));
    g.add_edge("a", END);
    let result = g.compile(CompileConfig::default());
    assert!(result.is_err());
    let err_msg = format!("{}", result.err().unwrap());
    assert!(err_msg.contains("No entry node"));
}

#[test]
fn compile_fail_entry_not_found() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("a"));
    g.add_edge(START, "nonexistent");
    g.add_edge("a", END);
    let result = g.compile(CompileConfig::default());
    assert!(result.is_err());
    let err_msg = format!("{}", result.err().unwrap());
    assert!(err_msg.contains("Entry node 'nonexistent' not found"));
}

#[test]
fn compile_fail_static_edge_to_nonexistent_node() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("a"));
    g.add_edge(START, "a");
    g.add_edge("a", "ghost");
    let result = g.compile(CompileConfig::default());
    assert!(result.is_err());
    let err_msg = format!("{}", result.err().unwrap());
    assert!(err_msg.contains("ghost"));
}

#[test]
fn compile_fail_edge_from_nonexistent_node() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("a"));
    g.add_edge(START, "a");
    g.add_edge("a", END);
    g.add_edge("phantom", "a");
    let result = g.compile(CompileConfig::default());
    assert!(result.is_err());
    let err_msg = format!("{}", result.err().unwrap());
    assert!(err_msg.contains("phantom"));
}

#[test]
fn compile_fail_node_without_outgoing_edge() {
    let mut g = StateGraph::new(make_channels());
    g.add_node(noop_node("a"));
    g.add_node(noop_node("orphan"));
    g.add_edge(START, "a");
    g.add_edge("a", END);
    let result = g.compile(CompileConfig::default());
    assert!(result.is_err());
    let err_msg = format!("{}", result.err().unwrap());
    assert!(err_msg.contains("orphan") && err_msg.contains("no outgoing edge"));
}
