use std::cell::RefCell;

use chess::update::credentials::{
    UpdateCredential, UpdateCredentialEnumStatus, UpdateCredentialError, UpdateCredentialStore,
    UpdateCredentials,
};

struct UpdateFixtureStore {
    held: RefCell<Option<String>>,
    unavailable: bool,
}

impl UpdateCredentialStore for UpdateFixtureStore {
    fn load(&self) -> Result<Option<UpdateCredential>, UpdateCredentialError> {
        self.held
            .borrow()
            .clone()
            .map(UpdateCredential::new)
            .transpose()
    }

    fn store(&self, credential: &UpdateCredential) -> Result<(), UpdateCredentialError> {
        if self.unavailable {
            return Err(UpdateCredentialError::SecureStorageUnavailable);
        }
        *self.held.borrow_mut() = Some(credential.expose().to_owned());
        Ok(())
    }

    fn clear(&self) -> Result<(), UpdateCredentialError> {
        *self.held.borrow_mut() = None;
        Ok(())
    }
}

fn credential() -> Result<UpdateCredential, UpdateCredentialError> {
    UpdateCredential::new("github_pat_fixture".to_owned())
}

#[test]
fn tokens_are_validated_and_never_printed() -> Result<(), UpdateCredentialError> {
    assert_eq!(
        UpdateCredential::new(String::new()),
        Err(UpdateCredentialError::Invalid)
    );
    assert_eq!(
        UpdateCredential::new("has space".to_owned()),
        Err(UpdateCredentialError::Invalid)
    );
    let shown = format!("{:?}", credential()?);
    assert!(!shown.contains("github_pat"));
    Ok(())
}

#[test]
fn unavailable_secure_storage_never_persists_and_offers_session_use()
-> Result<(), UpdateCredentialError> {
    let store = UpdateFixtureStore {
        held: RefCell::new(None),
        unavailable: true,
    };
    let mut credentials = UpdateCredentials::open(store);
    assert_eq!(credentials.status(), UpdateCredentialEnumStatus::Missing);
    assert_eq!(
        credentials.enter_persistent(credential()?),
        Err(UpdateCredentialError::SecureStorageUnavailable)
    );
    assert!(credentials.current().is_none());
    credentials.enter_session_only(credential()?);
    assert_eq!(
        credentials.status(),
        UpdateCredentialEnumStatus::SessionOnly
    );
    assert!(credentials.current().is_some());
    Ok(())
}

#[test]
fn a_rejected_credential_is_dropped_and_a_stored_one_reloads() -> Result<(), UpdateCredentialError>
{
    let store = UpdateFixtureStore {
        held: RefCell::new(None),
        unavailable: false,
    };
    let mut credentials = UpdateCredentials::open(store);
    credentials.enter_persistent(credential()?)?;
    assert_eq!(credentials.status(), UpdateCredentialEnumStatus::Persistent);
    credentials.mark_rejected();
    assert_eq!(credentials.status(), UpdateCredentialEnumStatus::Rejected);
    assert!(credentials.current().is_none());
    credentials.forget()?;
    assert_eq!(credentials.status(), UpdateCredentialEnumStatus::Missing);
    Ok(())
}
