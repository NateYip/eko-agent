# Checkpoints

Checkpoints persist graph state for fault tolerance and inspection.

## BaseCheckpointSaver Trait

Implement `BaseCheckpointSaver` to store and load checkpoints. The engine calls it after each superstep.

## MemorySaver

For tests and prototyping, use `MemorySaver`—an in-memory implementation that does not persist across process restarts.

## Auto-save Behavior

The engine **automatically saves** after each superstep. No extra configuration is required once a saver is provided.

## Benefits

- **Fault tolerance**: Restart from the last checkpoint after crashes.
- **State inspection**: Load checkpoints to debug or analyze graph state.
- **Resume**: Combine with interrupt/resume for human-in-the-loop workflows.
