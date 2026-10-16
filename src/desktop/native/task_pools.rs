use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};
use bevy::tasks::{AsyncComputeTaskPool, ComputeTaskPool, IoTaskPool};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DesktopNativeTaskPoolsError {
    #[error("The Bevy task pools have not been initialized")]
    Uninitialized,
    #[error("The Bevy task pool topology differs from the shared native capacity policy: {0:?}")]
    Topology([usize; 3]),
}

pub struct DesktopNativeTaskPools;

impl DesktopNativeTaskPools {
    pub const THREADS_PER_POOL: usize = 1;

    pub fn plugin() -> TaskPoolPlugin {
        TaskPoolPlugin {
            task_pool_options: TaskPoolOptions {
                io: Self::assignment(),
                async_compute: Self::assignment(),
                compute: Self::assignment(),
                ..TaskPoolOptions::default()
            },
        }
    }

    pub fn validate() -> Result<(), DesktopNativeTaskPoolsError> {
        let counts = [
            IoTaskPool::try_get()
                .ok_or(DesktopNativeTaskPoolsError::Uninitialized)?
                .thread_num(),
            AsyncComputeTaskPool::try_get()
                .ok_or(DesktopNativeTaskPoolsError::Uninitialized)?
                .thread_num(),
            ComputeTaskPool::try_get()
                .ok_or(DesktopNativeTaskPoolsError::Uninitialized)?
                .thread_num(),
        ];
        if counts == [Self::THREADS_PER_POOL; 3] {
            Ok(())
        } else {
            Err(DesktopNativeTaskPoolsError::Topology(counts))
        }
    }

    fn assignment() -> TaskPoolThreadAssignmentPolicy {
        TaskPoolThreadAssignmentPolicy {
            min_threads: Self::THREADS_PER_POOL,
            max_threads: Self::THREADS_PER_POOL,
            percent: 0.0,
            on_thread_spawn: None,
            on_thread_destroy: None,
        }
    }
}
