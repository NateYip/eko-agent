use std::collections::HashMap;

use serde_json::json;

use eko_core::checkpoint::{
    BaseCheckpointSaver, Checkpoint, CheckpointMetadata, CheckpointSource, PendingWrite,
};
use eko_core::MemorySaver;

fn make_checkpoint(id: &str, ts: &str) -> Checkpoint {
    Checkpoint {
        v: 1,
        id: id.to_string(),
        ts: ts.to_string(),
        channel_values: {
            let mut m = HashMap::new();
            m.insert("key".into(), json!(id));
            m
        },
        channel_versions: HashMap::new(),
        versions_seen: HashMap::new(),
        pending_sends: Vec::new(),
    }
}

fn make_metadata(step: i64, parent_id: Option<&str>) -> CheckpointMetadata {
    let mut parents = HashMap::new();
    if let Some(pid) = parent_id {
        parents.insert(String::new(), pid.to_string());
    }
    CheckpointMetadata {
        source: CheckpointSource::Loop,
        step,
        writes: HashMap::new(),
        parents,
    }
}

// =========================================================================
// put + get_tuple
// =========================================================================

#[tokio::test]
async fn put_and_get_by_id() {
    let saver = MemorySaver::new();

    let ckpt = make_checkpoint("ckpt-1", "2025-01-01T00:00:00Z");
    let meta = make_metadata(0, None);

    let id = saver.put("thread-1", ckpt, meta).await.unwrap();
    assert_eq!(id, "ckpt-1");

    let tuple = saver
        .get_tuple("thread-1", Some("ckpt-1"))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(tuple.thread_id, "thread-1");
    assert_eq!(tuple.checkpoint.id, "ckpt-1");
    assert_eq!(tuple.checkpoint.channel_values.get("key"), Some(&json!("ckpt-1")));
}

#[tokio::test]
async fn get_latest_without_id() {
    let saver = MemorySaver::new();

    saver
        .put(
            "thread-1",
            make_checkpoint("old", "2025-01-01T00:00:00Z"),
            make_metadata(0, None),
        )
        .await
        .unwrap();

    saver
        .put(
            "thread-1",
            make_checkpoint("new", "2025-01-02T00:00:00Z"),
            make_metadata(1, Some("old")),
        )
        .await
        .unwrap();

    let tuple = saver
        .get_tuple("thread-1", None)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(tuple.checkpoint.id, "new");
}

#[tokio::test]
async fn get_nonexistent_thread_returns_none() {
    let saver = MemorySaver::new();
    let result = saver.get_tuple("ghost", None).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn get_nonexistent_checkpoint_returns_none() {
    let saver = MemorySaver::new();
    saver
        .put(
            "t",
            make_checkpoint("c1", "2025-01-01T00:00:00Z"),
            make_metadata(0, None),
        )
        .await
        .unwrap();

    let result = saver.get_tuple("t", Some("c999")).await.unwrap();
    assert!(result.is_none());
}

// =========================================================================
// put_writes + pending_writes
// =========================================================================

#[tokio::test]
async fn put_writes_appears_in_tuple() {
    let saver = MemorySaver::new();
    saver
        .put(
            "t",
            make_checkpoint("c1", "2025-01-01T00:00:00Z"),
            make_metadata(0, None),
        )
        .await
        .unwrap();

    let writes = vec![PendingWrite {
        task_id: "task-1".into(),
        channel: "tool_node".into(),
        value: json!(null),
    }];
    saver.put_writes("t", "c1", writes, "task-1").await.unwrap();

    let tuple = saver.get_tuple("t", Some("c1")).await.unwrap().unwrap();
    assert_eq!(tuple.pending_writes.len(), 1);
    assert_eq!(tuple.pending_writes[0].channel, "tool_node");
}

#[tokio::test]
async fn multiple_put_writes_accumulate() {
    let saver = MemorySaver::new();
    saver
        .put(
            "t",
            make_checkpoint("c1", "2025-01-01T00:00:00Z"),
            make_metadata(0, None),
        )
        .await
        .unwrap();

    saver
        .put_writes(
            "t",
            "c1",
            vec![PendingWrite {
                task_id: "t1".into(),
                channel: "ch1".into(),
                value: json!(1),
            }],
            "t1",
        )
        .await
        .unwrap();

    saver
        .put_writes(
            "t",
            "c1",
            vec![PendingWrite {
                task_id: "t2".into(),
                channel: "ch2".into(),
                value: json!(2),
            }],
            "t2",
        )
        .await
        .unwrap();

    let tuple = saver.get_tuple("t", Some("c1")).await.unwrap().unwrap();
    assert_eq!(tuple.pending_writes.len(), 2);
}

// =========================================================================
// list
// =========================================================================

#[tokio::test]
async fn list_returns_descending_order() {
    let saver = MemorySaver::new();

    for i in 0..5 {
        let ts = format!("2025-01-0{}T00:00:00Z", i + 1);
        let parent = if i > 0 {
            Some(format!("c{}", i - 1))
        } else {
            None
        };
        saver
            .put(
                "t",
                make_checkpoint(&format!("c{i}"), &ts),
                make_metadata(i, parent.as_deref()),
            )
            .await
            .unwrap();
    }

    let list = saver.list("t", None).await.unwrap();
    assert_eq!(list.len(), 5);
    assert_eq!(list[0].checkpoint.id, "c4");
    assert_eq!(list[4].checkpoint.id, "c0");
}

#[tokio::test]
async fn list_with_limit() {
    let saver = MemorySaver::new();

    for i in 0..5 {
        let ts = format!("2025-01-0{}T00:00:00Z", i + 1);
        saver
            .put(
                "t",
                make_checkpoint(&format!("c{i}"), &ts),
                make_metadata(i, None),
            )
            .await
            .unwrap();
    }

    let list = saver.list("t", Some(2)).await.unwrap();
    assert_eq!(list.len(), 2);
}

#[tokio::test]
async fn list_empty_thread() {
    let saver = MemorySaver::new();
    let list = saver.list("nonexistent", None).await.unwrap();
    assert!(list.is_empty());
}

// =========================================================================
// delete_thread
// =========================================================================

#[tokio::test]
async fn delete_thread_removes_all() {
    let saver = MemorySaver::new();
    saver
        .put(
            "t",
            make_checkpoint("c1", "2025-01-01T00:00:00Z"),
            make_metadata(0, None),
        )
        .await
        .unwrap();

    saver
        .put_writes(
            "t",
            "c1",
            vec![PendingWrite {
                task_id: "t1".into(),
                channel: "ch".into(),
                value: json!(1),
            }],
            "t1",
        )
        .await
        .unwrap();

    saver.delete_thread("t").await.unwrap();

    assert!(saver.get_tuple("t", None).await.unwrap().is_none());
    assert!(saver.list("t", None).await.unwrap().is_empty());
}

#[tokio::test]
async fn delete_nonexistent_thread_is_ok() {
    let saver = MemorySaver::new();
    let result = saver.delete_thread("ghost").await;
    assert!(result.is_ok());
}

// =========================================================================
// 多线程隔离
// =========================================================================

#[tokio::test]
async fn different_threads_are_isolated() {
    let saver = MemorySaver::new();

    saver
        .put(
            "thread-a",
            make_checkpoint("ca", "2025-01-01T00:00:00Z"),
            make_metadata(0, None),
        )
        .await
        .unwrap();

    saver
        .put(
            "thread-b",
            make_checkpoint("cb", "2025-01-01T00:00:00Z"),
            make_metadata(0, None),
        )
        .await
        .unwrap();

    let a = saver.get_tuple("thread-a", None).await.unwrap().unwrap();
    let b = saver.get_tuple("thread-b", None).await.unwrap().unwrap();

    assert_eq!(a.checkpoint.id, "ca");
    assert_eq!(b.checkpoint.id, "cb");

    saver.delete_thread("thread-a").await.unwrap();
    assert!(saver.get_tuple("thread-a", None).await.unwrap().is_none());
    assert!(saver.get_tuple("thread-b", None).await.unwrap().is_some());
}
