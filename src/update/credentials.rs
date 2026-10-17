use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UpdateCredentialError {
    #[error("The update credential is empty, too long or contains whitespace or control characters")]
    Invalid,
    #[error("Secure credential storage is unavailable on this system")]
    SecureStorageUnavailable,
    #[error("Secure credential storage failed: {0}")]
    Storage(String),
}

#[derive(Clone, PartialEq, Eq)]
pub struct UpdateCredential(String);

impl UpdateCredential {
    pub const MAX_BYTES: usize = 4096;

    pub fn new(token: String) -> Result<Self, UpdateCredentialError> {
        if token.is_empty()
            || token.len() > Self::MAX_BYTES
            || token
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(UpdateCredentialError::Invalid);
        }
        Ok(Self(token))
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for UpdateCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("UpdateCredential(redacted)")
    }
}

pub trait UpdateCredentialStore {
    fn load(&self) -> Result<Option<UpdateCredential>, UpdateCredentialError>;
    fn store(&self, credential: &UpdateCredential) -> Result<(), UpdateCredentialError>;
    fn clear(&self) -> Result<(), UpdateCredentialError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateCredentialEnumStatus {
    Missing,
    Persistent,
    SessionOnly,
    Rejected,
}

pub struct UpdateCredentials<S: UpdateCredentialStore> {
    store: S,
    active: Option<UpdateCredential>,
    status: UpdateCredentialEnumStatus,
}

impl<S: UpdateCredentialStore> UpdateCredentials<S> {
    pub fn open(store: S) -> Self {
        match store.load() {
            Ok(Some(credential)) => Self {
                store,
                active: Some(credential),
                status: UpdateCredentialEnumStatus::Persistent,
            },
            Ok(None) | Err(_) => Self {
                store,
                active: None,
                status: UpdateCredentialEnumStatus::Missing,
            },
        }
    }

    pub const fn status(&self) -> UpdateCredentialEnumStatus {
        self.status
    }

    pub fn current(&self) -> Option<&UpdateCredential> {
        self.active.as_ref()
    }

    pub fn enter_persistent(
        &mut self,
        credential: UpdateCredential,
    ) -> Result<(), UpdateCredentialError> {
        self.store.store(&credential)?;
        self.active = Some(credential);
        self.status = UpdateCredentialEnumStatus::Persistent;
        Ok(())
    }

    pub fn enter_session_only(&mut self, credential: UpdateCredential) {
        self.active = Some(credential);
        self.status = UpdateCredentialEnumStatus::SessionOnly;
    }

    pub fn mark_rejected(&mut self) {
        self.active = None;
        self.status = UpdateCredentialEnumStatus::Rejected;
    }

    pub fn forget(&mut self) -> Result<(), UpdateCredentialError> {
        self.store.clear()?;
        self.active = None;
        self.status = UpdateCredentialEnumStatus::Missing;
        Ok(())
    }
}
