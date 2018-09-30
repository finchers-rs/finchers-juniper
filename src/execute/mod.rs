//! GraphQL executors.
//! 
//! In order to choose the strategy for executing GraphQL queries,
//! the following GraphQL executors are provided:
//! 
//! * [`Nonblocking`]  
//!   It spawns the task for executing GraphQL queries by using the Tokio's
//!   default executor.  It also notify the start of the blocking section to
//!   the runtime by using the blocking API provided by `tokio_threadpool`.
//! 
//! * [`CurrentThread`]  
//!   It executes the GraphQL queries on the current thread.
//! 
//! * [`WithSpawner`]  
//!   It spawn the task for executing GraphQL queries by using the specified
//!   task executor. Unlike to `Nonblocking`, it does not notify the start of
//!   blocking section to the runtime.
//! 
//! [`Nonblocking`]: ./struct.Nonblocking.html
//! [`CurrentThread`]: ./struct.CurrentThread.html
//! [`WithSpawner`]: ./struct.WithSpawner.html

mod current_thread;
mod nonblocking;
mod with_spawner;

pub use self::current_thread::{current_thread, CurrentThread};
pub use self::nonblocking::{nonblocking, Nonblocking};
pub use self::with_spawner::{with_spawner, WithSpawner};
