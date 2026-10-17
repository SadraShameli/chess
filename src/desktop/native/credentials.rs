use crate::update::credentials::{UpdateCredential, UpdateCredentialError, UpdateCredentialStore};

pub struct DesktopNativeCredentials {
    service: String,
    account: String,
}

impl DesktopNativeCredentials {
    pub const SERVICE: &'static str = "chess-update";
    pub const ACCOUNT: &'static str = "repository-contents-read";

    pub fn new() -> Self {
        Self {
            service: Self::SERVICE.to_owned(),
            account: Self::ACCOUNT.to_owned(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, UpdateCredentialError> {
        keyring::Entry::new(&self.service, &self.account).map_err(Self::classify)
    }

    fn classify(error: keyring::Error) -> UpdateCredentialError {
        if matches!(
            error,
            keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_)
        ) {
            UpdateCredentialError::SecureStorageUnavailable
        } else {
            UpdateCredentialError::Storage(error.to_string())
        }
    }
}

impl Default for DesktopNativeCredentials {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateCredentialStore for DesktopNativeCredentials {
    fn load(&self) -> Result<Option<UpdateCredential>, UpdateCredentialError> {
        match self.entry()?.get_password() {
            Ok(token) => UpdateCredential::new(token).map(Some),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(Self::classify(error)),
        }
    }

    fn store(&self, credential: &UpdateCredential) -> Result<(), UpdateCredentialError> {
        self.entry()?
            .set_password(credential.expose())
            .map_err(Self::classify)
    }

    fn clear(&self) -> Result<(), UpdateCredentialError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(Self::classify(error)),
        }
    }
}
