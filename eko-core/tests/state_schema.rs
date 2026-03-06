use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use eko_core::channels::ChannelValues;
use eko_core::graph::command::NodeOutput;
use eko_core::graph::edge::{END, START};
use eko_core::{CompileConfig, GraphConfig, GraphResult, State, StateGraph};

// =========================================================================
// derive(State) 基础功能
// =========================================================================

fn append_reducer(left: Value, right: Value) -> anyhow::Result<Value> {
    let mut arr: Vec<Value> = serde_json::from_value(left)?;
    let new: Vec<Value> = serde_json::from_value(right)?;
    arr.extend(new);
    Ok(serde_json::to_value(arr)?)
}

#[derive(eko_core::State, Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestState {
    pub query: String,

    #[state(reducer = "append_reducer")]
    pub items: Vec<String>,

    #[state(ephemeral)]
    pub scratch: Option<String>,
}

#[test]
fn state_channels_generates_correct_types() {
    let channels = TestState::channels();
    assert!(channels.contains_key("query"));
    assert!(channels.contains_key("items"));
    assert!(channels.contains_key("scratch"));
    assert_eq!(channels.len(), 3);

    // scratch 是 ephemeral，不持久化
    assert!(!channels["scratch"].is_persistent());
    // query 和 items 持久化
    assert!(channels["query"].is_persistent());
    assert!(channels["items"].is_persistent());
}

#[test]
fn state_keys_returns_all_field_names() {
    let keys = TestState::keys();
    assert_eq!(keys, vec!["query", "items", "scratch"]);
}

#[test]
fn state_roundtrip_channel_values() {
    let state = TestState {
        query: "hello".into(),
        items: vec!["a".into(), "b".into()],
        scratch: Some("tmp".into()),
    };

    let values = state.to_channel_values();
    assert_eq!(values.get("query"), Some(&json!("hello")));
    assert_eq!(values.get("items"), Some(&json!(["a", "b"])));
    assert_eq!(values.get("scratch"), Some(&json!("tmp")));

    let restored = TestState::from_channel_values(&values).unwrap();
    assert_eq!(restored, state);
}

#[test]
fn state_from_channel_values_with_missing_fields() {
    let values = ChannelValues::new();
    let state = TestState::from_channel_values(&values).unwrap();
    assert_eq!(state.query, "");
    assert!(state.items.is_empty());
    assert_eq!(state.scratch, None);
}

// =========================================================================
// StateGraph::from_state
// =========================================================================

fn default_config() -> GraphConfig {
    GraphConfig {
        thread_id: Some("test-thread".into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn from_state_creates_graph_with_correct_channels() {
    let mut g = StateGraph::from_state::<TestState>();

    g.add_node_fn("process", |state, _ctx| async move {
        let s = TestState::from_channel_values(&state)?;
        let mut out = ChannelValues::new();
        out.insert("items".into(), json!(vec![format!("processed:{}", s.query)]));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "process");
    g.add_edge("process", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();

    let mut input = ChannelValues::new();
    input.insert("query".into(), json!("test"));

    let result = compiled.invoke(input, &default_config()).await.unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(
                values.get("items"),
                Some(&json!(["processed:test"]))
            );
        }
        other => panic!("Expected Done, got: {other:?}"),
    }
}

// =========================================================================
// add_node_fn_typed
// =========================================================================

#[tokio::test]
async fn typed_node_receives_strong_typed_state() {
    let mut g = StateGraph::from_state::<TestState>();

    g.add_node_fn_typed::<TestState, _, _>("process", |s, _ctx| async move {
        let mut out = ChannelValues::new();
        out.insert("items".into(), json!(vec![format!("typed:{}", s.query)]));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "process");
    g.add_edge("process", END);

    let compiled = g.compile(CompileConfig::default()).unwrap();

    let mut input = ChannelValues::new();
    input.insert("query".into(), json!("hello"));

    let result = compiled.invoke(input, &default_config()).await.unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("items"), Some(&json!(["typed:hello"])));
        }
        other => panic!("Expected Done, got: {other:?}"),
    }
}

// =========================================================================
// Input/Output Schema
// =========================================================================

#[derive(eko_core::State, Debug, Clone, Serialize, Deserialize)]
struct FullState {
    pub query: String,
    pub internal_scratch: String,
    pub answer: String,
}

#[derive(eko_core::State, Debug, Clone, Serialize, Deserialize)]
struct InputSchema {
    pub query: String,
}

#[derive(eko_core::State, Debug, Clone, Serialize, Deserialize)]
struct OutputSchema {
    pub answer: String,
}

#[tokio::test]
async fn input_keys_filters_extra_fields() {
    let mut g = StateGraph::from_state::<FullState>();

    g.add_node_fn("process", |state, _ctx| async move {
        let query = state
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        // internal_scratch should NOT have been set from input
        let scratch = state
            .get("internal_scratch")
            .and_then(|v| v.as_str())
            .unwrap_or("empty")
            .to_string();
        let mut out = ChannelValues::new();
        out.insert("answer".into(), json!(format!("{query}:{scratch}")));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "process");
    g.add_edge("process", END);

    let compiled = g
        .with_input_keys(&["query"])
        .compile(CompileConfig::default())
        .unwrap();

    let mut input = ChannelValues::new();
    input.insert("query".into(), json!("hello"));
    input.insert("internal_scratch".into(), json!("SHOULD_BE_IGNORED"));

    let result = compiled.invoke(input, &default_config()).await.unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            // scratch should be "empty" because "internal_scratch" was filtered out
            assert_eq!(values.get("answer"), Some(&json!("hello:empty")));
        }
        other => panic!("Expected Done, got: {other:?}"),
    }
}

#[tokio::test]
async fn output_keys_filters_internal_fields() {
    let mut g = StateGraph::from_state::<FullState>();

    g.add_node_fn("process", |state, _ctx| async move {
        let query = state
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut out = ChannelValues::new();
        out.insert("internal_scratch".into(), json!("secret_data"));
        out.insert("answer".into(), json!(format!("answer:{query}")));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "process");
    g.add_edge("process", END);

    let compiled = g
        .with_output_keys(&["answer"])
        .compile(CompileConfig::default())
        .unwrap();

    let mut input = ChannelValues::new();
    input.insert("query".into(), json!("test"));

    let result = compiled.invoke(input, &default_config()).await.unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("answer"), Some(&json!("answer:test")));
            // internal_scratch should not be in the output
            assert!(
                values.get("internal_scratch").is_none(),
                "internal_scratch should be filtered out"
            );
            // query should also not be in the output
            assert!(
                values.get("query").is_none(),
                "query should be filtered out"
            );
        }
        other => panic!("Expected Done, got: {other:?}"),
    }
}

#[tokio::test]
async fn with_input_output_state_types() {
    let mut g = StateGraph::from_state::<FullState>();

    g.add_node_fn("process", |state, _ctx| async move {
        let query = state
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let mut out = ChannelValues::new();
        out.insert("internal_scratch".into(), json!("temp"));
        out.insert("answer".into(), json!(format!("done:{query}")));
        Ok(NodeOutput::Update(out))
    });

    g.add_edge(START, "process");
    g.add_edge("process", END);

    let compiled = g
        .with_input::<InputSchema>()
        .with_output::<OutputSchema>()
        .compile(CompileConfig::default())
        .unwrap();

    let mut input = ChannelValues::new();
    input.insert("query".into(), json!("world"));
    input.insert("answer".into(), json!("SHOULD_BE_IGNORED"));

    let result = compiled.invoke(input, &default_config()).await.unwrap();

    match result {
        GraphResult::Done { values, .. } => {
            assert_eq!(values.get("answer"), Some(&json!("done:world")));
            assert!(values.get("query").is_none());
            assert!(values.get("internal_scratch").is_none());
        }
        other => panic!("Expected Done, got: {other:?}"),
    }
}
