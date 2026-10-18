#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneEnumLifecycle {
    Initializing,
    Ready,
    Failed,
    Retrying,
}
