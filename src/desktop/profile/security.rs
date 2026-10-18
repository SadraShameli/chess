#[derive(Debug, Clone)]
pub struct DesktopProfileSecurity {
    #[cfg(unix)]
    pub(super) effective_uid: u32,
    #[cfg(windows)]
    pub(super) user_sid: String,
}

impl DesktopProfileSecurity {
    #[cfg(unix)]
    pub(crate) fn effective_user_id(&self) -> u32 {
        self.effective_uid
    }

    #[cfg(windows)]
    pub(crate) fn user_sid(&self) -> &str {
        &self.user_sid
    }
}
