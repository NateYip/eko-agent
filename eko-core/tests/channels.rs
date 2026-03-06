use serde_json::{json, Value};

use eko_core::channels::base::AnyChannel;
use eko_core::channels::{
    BinaryOperatorAggregate, DynamicBarrierValue, EphemeralValue, LastValue, NamedBarrierValue,
    Topic,
};

// =========================================================================
// LastValue
// =========================================================================

#[test]
fn last_value_empty_by_default() {
    let ch = LastValue::new();
    assert!(ch.get().is_none());
    assert!(ch.checkpoint().is_none());
}

#[test]
fn last_value_single_update() {
    let mut ch = LastValue::new();
    let changed = ch.update(vec![json!(42)]).unwrap();
    assert!(changed);
    assert_eq!(ch.get(), Some(json!(42)));
}

#[test]
fn last_value_takes_last_from_batch() {
    let mut ch = LastValue::new();
    ch.update(vec![json!(1), json!(2), json!(3)]).unwrap();
    assert_eq!(ch.get(), Some(json!(3)));
}

#[test]
fn last_value_empty_batch_no_change() {
    let mut ch = LastValue::new();
    let changed = ch.update(vec![]).unwrap();
    assert!(!changed);
    assert!(ch.get().is_none());
}

#[test]
fn last_value_checkpoint_matches_get() {
    let mut ch = LastValue::new();
    ch.update(vec![json!("hello")]).unwrap();
    assert_eq!(ch.get(), ch.checkpoint());
}

#[test]
fn last_value_from_checkpoint_restores() {
    let ch = LastValue::new();
    let restored = ch.from_checkpoint(Some(json!(99)));
    assert_eq!(restored.get(), Some(json!(99)));
}

#[test]
fn last_value_from_checkpoint_none() {
    let ch = LastValue::new();
    let restored = ch.from_checkpoint(None);
    assert!(restored.get().is_none());
}

// =========================================================================
// BinaryOperatorAggregate
// =========================================================================

fn append_reducer(existing: Value, new: Value) -> anyhow::Result<Value> {
    match existing {
        Value::Array(mut arr) => {
            arr.push(new);
            Ok(Value::Array(arr))
        }
        _ => Ok(Value::Array(vec![existing, new])),
    }
}

#[test]
fn binop_empty_by_default() {
    let ch = BinaryOperatorAggregate::new(append_reducer);
    assert!(ch.get().is_none());
}

#[test]
fn binop_first_write_sets_value() {
    let mut ch = BinaryOperatorAggregate::new(append_reducer);
    ch.update(vec![json!("a")]).unwrap();
    assert_eq!(ch.get(), Some(json!("a")));
}

#[test]
fn binop_reducer_accumulates() {
    let mut ch = BinaryOperatorAggregate::new(append_reducer);
    ch.update(vec![json!([1])]).unwrap();
    ch.update(vec![json!(2)]).unwrap();
    ch.update(vec![json!(3)]).unwrap();
    // [1] is set directly, then reducer pushes 2 and 3 into the array
    assert_eq!(ch.get(), Some(json!([1, 2, 3])));
}

#[test]
fn binop_batch_update_applies_sequentially() {
    let mut ch = BinaryOperatorAggregate::new(append_reducer);
    // [] is set directly, then reducer pushes 1 and 2 into the array
    ch.update(vec![json!([]), json!(1), json!(2)]).unwrap();
    assert_eq!(ch.get(), Some(json!([1, 2])));
}

#[test]
fn binop_empty_batch_no_change() {
    let mut ch = BinaryOperatorAggregate::new(append_reducer);
    let changed = ch.update(vec![]).unwrap();
    assert!(!changed);
}

#[test]
fn binop_checkpoint_matches_get() {
    let mut ch = BinaryOperatorAggregate::new(append_reducer);
    ch.update(vec![json!(42)]).unwrap();
    assert_eq!(ch.get(), ch.checkpoint());
}

#[test]
fn binop_from_checkpoint_preserves_reducer() {
    let ch = BinaryOperatorAggregate::new(append_reducer);
    let mut restored = ch.from_checkpoint(Some(json!([1, 2])));
    assert_eq!(restored.get(), Some(json!([1, 2])));
    restored.update(vec![json!(3)]).unwrap();
    assert_eq!(restored.get(), Some(json!([1, 2, 3])));
}

// =========================================================================
// EphemeralValue
// =========================================================================

#[test]
fn ephemeral_empty_by_default() {
    let ch = EphemeralValue::new();
    assert!(ch.get().is_none());
}

#[test]
fn ephemeral_update_and_get() {
    let mut ch = EphemeralValue::new();
    ch.update(vec![json!("temp")]).unwrap();
    assert_eq!(ch.get(), Some(json!("temp")));
}

#[test]
fn ephemeral_takes_last_from_batch() {
    let mut ch = EphemeralValue::new();
    ch.update(vec![json!(1), json!(2)]).unwrap();
    assert_eq!(ch.get(), Some(json!(2)));
}

#[test]
fn ephemeral_checkpoint_returns_none() {
    let mut ch = EphemeralValue::new();
    ch.update(vec![json!(42)]).unwrap();
    assert!(ch.checkpoint().is_none());
}

#[test]
fn ephemeral_not_persistent() {
    let ch = EphemeralValue::new();
    assert!(!ch.is_persistent());
}

#[test]
fn ephemeral_consume_takes_value() {
    let mut ch = EphemeralValue::new();
    ch.update(vec![json!("data")]).unwrap();
    let val = ch.consume();
    assert_eq!(val, Some(json!("data")));
    assert!(ch.get().is_none());
}

#[test]
fn ephemeral_from_checkpoint_always_empty() {
    let ch = EphemeralValue::new();
    let restored = ch.from_checkpoint(Some(json!("should_be_ignored")));
    assert!(restored.get().is_none());
}

// =========================================================================
// Topic
// =========================================================================

#[test]
fn topic_empty_by_default() {
    let ch = Topic::new();
    assert!(ch.get().is_none());
}

#[test]
fn topic_appends_values() {
    let mut ch = Topic::new();
    ch.update(vec![json!(1)]).unwrap();
    ch.update(vec![json!(2), json!(3)]).unwrap();
    assert_eq!(ch.get(), Some(json!([1, 2, 3])));
}

#[test]
fn topic_empty_batch_no_change() {
    let mut ch = Topic::new();
    let changed = ch.update(vec![]).unwrap();
    assert!(!changed);
}

#[test]
fn topic_drain_takes_all() {
    let mut ch = Topic::new();
    ch.update(vec![json!("a"), json!("b")]).unwrap();
    let drained = ch.drain();
    assert_eq!(drained, vec![json!("a"), json!("b")]);
    assert!(ch.get().is_none());
}

#[test]
fn topic_checkpoint_returns_none() {
    let mut ch = Topic::new();
    ch.update(vec![json!(1)]).unwrap();
    assert!(ch.checkpoint().is_none());
}

#[test]
fn topic_not_persistent() {
    let ch = Topic::new();
    assert!(!ch.is_persistent());
}

#[test]
fn topic_from_checkpoint_always_empty() {
    let ch = Topic::new();
    let restored = ch.from_checkpoint(Some(json!([1, 2, 3])));
    assert!(restored.get().is_none());
}

// =========================================================================
// NamedBarrierValue
// =========================================================================

#[test]
fn barrier_empty_by_default() {
    let ch = NamedBarrierValue::new(&["a", "b"]);
    assert!(ch.get().is_none());
}

#[test]
fn barrier_not_satisfied_with_partial() {
    let mut ch = NamedBarrierValue::new(&["a", "b"]);
    let changed = ch.update(vec![json!("a")]).unwrap();
    assert!(changed);
    assert!(ch.get().is_none());
}

#[test]
fn barrier_satisfied_when_all_seen() {
    let mut ch = NamedBarrierValue::new(&["a", "b"]);
    ch.update(vec![json!("a")]).unwrap();
    ch.update(vec![json!("b")]).unwrap();
    assert!(ch.get().is_some());
}

#[test]
fn barrier_ignores_unknown_names() {
    let mut ch = NamedBarrierValue::new(&["a", "b"]);
    let changed = ch.update(vec![json!("c")]).unwrap();
    assert!(!changed);
    assert!(ch.get().is_none());
}

#[test]
fn barrier_duplicate_update_no_change() {
    let mut ch = NamedBarrierValue::new(&["a", "b"]);
    ch.update(vec![json!("a")]).unwrap();
    let changed = ch.update(vec![json!("a")]).unwrap();
    assert!(!changed);
    assert!(ch.get().is_none());
}

#[test]
fn barrier_not_persistent() {
    let ch = NamedBarrierValue::new(&["a"]);
    assert!(!ch.is_persistent());
    assert!(ch.checkpoint().is_none());
}

#[test]
fn barrier_from_checkpoint_resets() {
    let mut ch = NamedBarrierValue::new(&["a", "b"]);
    ch.update(vec![json!("a"), json!("b")]).unwrap();
    assert!(ch.get().is_some());

    let reset = ch.from_checkpoint(None);
    assert!(reset.get().is_none());
}

#[test]
fn barrier_batch_update_multiple_names() {
    let mut ch = NamedBarrierValue::new(&["x", "y"]);
    let changed = ch.update(vec![json!("x"), json!("y")]).unwrap();
    assert!(changed);
    assert!(ch.get().is_some());
}

// =========================================================================
// DynamicBarrierValue
// =========================================================================

#[test]
fn dynamic_barrier_empty_by_default() {
    let ch = DynamicBarrierValue::new();
    assert!(ch.get().is_none());
}

#[test]
fn dynamic_barrier_not_persistent() {
    let ch = DynamicBarrierValue::new();
    assert!(!ch.is_persistent());
    assert!(ch.checkpoint().is_none());
}

#[test]
fn dynamic_barrier_set_names_then_satisfy() {
    let mut ch = DynamicBarrierValue::new();
    // priming: set wait target
    ch.update(vec![json!({"__wait_for__": ["a", "b"]})]).unwrap();
    assert!(ch.get().is_none());

    ch.update(vec![json!("a")]).unwrap();
    assert!(ch.get().is_none());

    ch.update(vec![json!("b")]).unwrap();
    assert!(ch.get().is_some());
}

#[test]
fn dynamic_barrier_ignores_signals_before_priming() {
    let mut ch = DynamicBarrierValue::new();
    let changed = ch.update(vec![json!("a")]).unwrap();
    assert!(!changed);
    assert!(ch.get().is_none());
}

#[test]
fn dynamic_barrier_ignores_unknown_signals() {
    let mut ch = DynamicBarrierValue::new();
    ch.update(vec![json!({"__wait_for__": ["a"]})]).unwrap();
    let changed = ch.update(vec![json!("unknown")]).unwrap();
    assert!(!changed);
    assert!(ch.get().is_none());
}

#[test]
fn dynamic_barrier_reset_on_new_wait_for() {
    let mut ch = DynamicBarrierValue::new();
    ch.update(vec![json!({"__wait_for__": ["a"]})]).unwrap();
    ch.update(vec![json!("a")]).unwrap();
    assert!(ch.get().is_some());

    // Re-prime with different targets resets
    ch.update(vec![json!({"__wait_for__": ["x", "y"]})]).unwrap();
    assert!(ch.get().is_none());
}

#[test]
fn dynamic_barrier_from_checkpoint_resets() {
    let mut ch = DynamicBarrierValue::new();
    ch.update(vec![json!({"__wait_for__": ["a"]})]).unwrap();
    ch.update(vec![json!("a")]).unwrap();
    assert!(ch.get().is_some());

    let reset = ch.from_checkpoint(None);
    assert!(reset.get().is_none());
}

// =========================================================================
// Overwrite mechanism
// =========================================================================

#[test]
fn binop_overwrite_bypasses_reducer() {
    let mut ch = BinaryOperatorAggregate::new(|a: Value, b: Value| {
        let mut arr = a.as_array().cloned().unwrap_or_default();
        arr.push(b);
        Ok(Value::Array(arr))
    });

    // Seed with an array so reducer appends correctly
    ch.update(vec![json!(["first"])]).unwrap();
    ch.update(vec![json!("second")]).unwrap();
    assert_eq!(ch.get(), Some(json!(["first", "second"])));

    // Overwrite bypasses reducer entirely
    ch.update(vec![json!({"__overwrite__": "replaced"})]).unwrap();
    assert_eq!(ch.get(), Some(json!("replaced")));
}

#[test]
fn binop_overwrite_then_normal_continues_reducing() {
    let mut ch = BinaryOperatorAggregate::new(|a: Value, b: Value| {
        let mut arr = a.as_array().cloned().unwrap_or_default();
        arr.push(b);
        Ok(Value::Array(arr))
    });

    ch.update(vec![json!({"__overwrite__": json!(["base"])})]).unwrap();
    assert_eq!(ch.get(), Some(json!(["base"])));

    ch.update(vec![json!("added")]).unwrap();
    assert_eq!(ch.get(), Some(json!(["base", "added"])));
}

#[test]
fn overwrite_helper_constructs_correct_value() {
    let val = eko_core::overwrite(json!(42));
    assert_eq!(val, json!({"__overwrite__": 42}));
}
