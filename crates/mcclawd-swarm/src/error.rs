use thiserror::Error;

#[derive(Debug, Error)]
pub enum SwarmError {
    #[error("Planning failed: {0}")]
    PlanningFailed(String),

    #[error("Worker failed: subtask {subtask_id} — {message}")]
    WorkerFailed { subtask_id: String, message: String },

    #[error("DAG cycle detected")]
    CycleDetected,

    #[error("Merge failed: {0}")]
    MergeFailed(String),

    #[error("Timeout: worker exceeded {0:?}")]
    Timeout(std::time::Duration),

    #[error("Max replan depth exceeded: {0}")]
    MaxReplanDepth(u32),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, SwarmError>;
