//! 基于内存的检查点存储后端。
//!
//! 使用 `HashMap` + `Mutex` 实现，所有数据存活于进程生命周期内。
//! 主要用于测试和开发环境，生产环境推荐使用 `SqliteSaver`。

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::saver::BaseCheckpointSaver;
use super::types::{Checkpoint, CheckpointMetadata, CheckpointTuple, PendingWrite};

// (checkpoint 字节, metadata 字节, 父检查点 ID)
type StorageEntry = (Vec<u8>, Vec<u8>, Option<String>);

type PendingWriteEntries = Vec<(String, String, Vec<u8>)>;

/// 内存检查点存储，数据不会持久化到磁盘
pub struct MemorySaver {
    /// thread_id -> checkpoint_id -> (checkpoint, metadata, parent_id)
    storage: Mutex<HashMap<String, HashMap<String, StorageEntry>>>,
    /// thread_id -> checkpoint_id -> [(task_id, channel, value_bytes)]
    writes: Mutex<HashMap<String, HashMap<String, PendingWriteEntries>>>,
}

impl Default for MemorySaver {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySaver {
    pub fn new() -> Self {
        Self {
            storage: Mutex::new(HashMap::new()),
            writes: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl BaseCheckpointSaver for MemorySaver {
    async fn get_tuple(
        &self,
        thread_id: &str,
        checkpoint_id: Option<&str>,
    ) -> anyhow::Result<Option<CheckpointTuple>> {
        let store = self.storage.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let thread_store = match store.get(thread_id) {
            Some(ts) => ts,
            None => return Ok(None),
        };

        let entry = if let Some(cid) = checkpoint_id {
            thread_store.get(cid)
        } else {
            // 未指定 ID 时按时间戳找最新的检查点
            thread_store
                .iter()
                .max_by_key(|(_, (ckpt_bytes, _, _))| {
                    serde_json::from_slice::<Checkpoint>(ckpt_bytes)
                        .map(|c| c.ts.clone())
                        .unwrap_or_default()
                })
                .map(|(_, v)| v)
        };

        let Some((ckpt_bytes, meta_bytes, parent_id)) = entry else {
            return Ok(None);
        };

        let checkpoint: Checkpoint = serde_json::from_slice(ckpt_bytes)?;
        let metadata: CheckpointMetadata = serde_json::from_slice(meta_bytes)?;

        let pending_writes = {
            let ws = self.writes.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
            ws.get(thread_id)
                .and_then(|tw| tw.get(&checkpoint.id))
                .map(|writes| {
                    writes
                        .iter()
                        .map(|(task_id, channel, value_bytes)| PendingWrite {
                            task_id: task_id.clone(),
                            channel: channel.clone(),
                            value: serde_json::from_slice(value_bytes).unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        Ok(Some(CheckpointTuple {
            thread_id: thread_id.to_string(),
            checkpoint,
            metadata,
            parent_checkpoint_id: parent_id.clone(),
            pending_writes,
        }))
    }

    async fn put(
        &self,
        thread_id: &str,
        checkpoint: Checkpoint,
        metadata: CheckpointMetadata,
    ) -> anyhow::Result<String> {
        let cid = checkpoint.id.clone();
        let parent_id = metadata.parents.get("").cloned();
        let ckpt_bytes = serde_json::to_vec(&checkpoint)?;
        let meta_bytes = serde_json::to_vec(&metadata)?;

        let mut store = self.storage.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        store
            .entry(thread_id.to_string())
            .or_default()
            .insert(cid.clone(), (ckpt_bytes, meta_bytes, parent_id));

        Ok(cid)
    }

    async fn put_writes(
        &self,
        thread_id: &str,
        checkpoint_id: &str,
        writes: Vec<PendingWrite>,
        task_id: &str,
    ) -> anyhow::Result<()> {
        let mut ws = self.writes.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let thread_writes = ws.entry(thread_id.to_string()).or_default();
        let ckpt_writes = thread_writes.entry(checkpoint_id.to_string()).or_default();

        for (idx, w) in writes.into_iter().enumerate() {
            let _ = idx;
            let value_bytes = serde_json::to_vec(&w.value)?;
            ckpt_writes.push((task_id.to_string(), w.channel, value_bytes));
        }
        Ok(())
    }

    async fn list(
        &self,
        thread_id: &str,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<CheckpointTuple>> {
        let store = self.storage.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let thread_store = match store.get(thread_id) {
            Some(ts) => ts,
            None => return Ok(Vec::new()),
        };

        let mut tuples: Vec<CheckpointTuple> = thread_store
            .iter()
            .filter_map(|(_, (ckpt_bytes, meta_bytes, parent_id))| {
                let checkpoint: Checkpoint = serde_json::from_slice(ckpt_bytes).ok()?;
                let metadata: CheckpointMetadata = serde_json::from_slice(meta_bytes).ok()?;
                Some(CheckpointTuple {
                    thread_id: thread_id.to_string(),
                    checkpoint,
                    metadata,
                    parent_checkpoint_id: parent_id.clone(),
                    pending_writes: Vec::new(),
                })
            })
            .collect();

        tuples.sort_by(|a, b| b.checkpoint.ts.cmp(&a.checkpoint.ts));
        if let Some(n) = limit {
            tuples.truncate(n);
        }
        Ok(tuples)
    }

    async fn delete_thread(&self, thread_id: &str) -> anyhow::Result<()> {
        self.storage
            .lock()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .remove(thread_id);
        self.writes
            .lock()
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .remove(thread_id);
        Ok(())
    }
}
