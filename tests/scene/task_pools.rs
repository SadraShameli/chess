use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};

pub(crate) struct SceneTaskPoolFixture;

impl SceneTaskPoolFixture {
    pub(crate) const THREADS_PER_POOL: usize = 1;

    pub(crate) fn plugin() -> TaskPoolPlugin {
        TaskPoolPlugin {
            task_pool_options: TaskPoolOptions {
                io: Self::assignment(),
                async_compute: Self::assignment(),
                compute: Self::assignment(),
                ..TaskPoolOptions::default()
            },
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
