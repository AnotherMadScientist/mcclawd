//! McClawd tasks — task lifecycle

pub mod manager;
pub mod scheduler;
pub mod swarm;
pub use manager::TaskManager;
pub use scheduler::TaskScheduler;
