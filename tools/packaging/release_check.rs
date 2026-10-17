use chess::update::release::{UpdateReleaseCheck, UpdateReleaseCheckError};

pub(crate) struct ToolsPackagingReleaseCheck;

impl ToolsPackagingReleaseCheck {
    pub(crate) fn run(tag: &str) -> Result<(), UpdateReleaseCheckError> {
        let version = env!("CARGO_PKG_VERSION");
        UpdateReleaseCheck::versions_match(tag, version, version, version)
    }
}
