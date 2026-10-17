use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use chess::clock::duration::ClockInstant;
use chess::clock::time_control::ClockTimeControl;
use chess::desktop::identity::DesktopRequestId;
use chess::desktop::native::event_loop::DesktopNativeEventLoopWake;
use chess::desktop::profile::activation::{
    DesktopProfileActivationBinding, DesktopProfileActivationEnumResult,
    DesktopProfileActivationWindow,
};
use chess::desktop::profile::owner_record::{
    DesktopProfileEnumPhase, DesktopProfileOwnerRecord, DesktopProfileOwnerRecordBoundEndpoint,
    DesktopProfileOwnerRecordEndpoint, DesktopProfileOwnerRecordError,
    DesktopProfileOwnerRecordPublisher,
};
use chess::desktop::profile::{
    DesktopProfileAdmission, DesktopProfileEnumAdmission, DesktopProfileId, DesktopProfileLease,
    DesktopProfileOsLock, DesktopProfileSecurity,
};
use chess::game::enums::GameEnumPhase;
use chess::game::library_organization::GameLibraryOrganizationTimestamp;
use chess::game::storage_repository::{
    GameStorageContentEdit, GameStorageEnumContentChange, GameStorageEnumEditCause,
    GameStorageRepository, GameStorageRevision, GameStorageSaveRequest,
};
use chess::storage::library_played::StorageLibraryPlayedRepository;
use tempfile::TempDir;

use crate::desktop_game_supervisor_storage::DesktopGameSupervisorFixtureStorage;
use crate::game_transition_fixture::GameTransitionFixture;

type DesktopRepeatedLaunchResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct DesktopRepeatedLaunchEndpoint {
    binding: DesktopProfileActivationBinding,
    endpoint: DesktopProfileOwnerRecordEndpoint,
}

impl DesktopRepeatedLaunchEndpoint {
    fn new(binding: DesktopProfileActivationBinding) -> DesktopRepeatedLaunchResult<Self> {
        #[cfg(unix)]
        let endpoint =
            DesktopProfileOwnerRecordEndpoint::unix_socket(Path::new("/tmp/chess-repeated.sock"))?;
        #[cfg(windows)]
        let endpoint = DesktopProfileOwnerRecordEndpoint::windows_pipe(r"\\.\pipe\chess-repeated")?;
        Ok(Self { binding, endpoint })
    }
}

impl DesktopProfileOwnerRecordBoundEndpoint for DesktopRepeatedLaunchEndpoint {
    fn confirm_bound(
        &self,
        binding: DesktopProfileActivationBinding,
    ) -> Result<DesktopProfileOwnerRecordEndpoint, DesktopProfileOwnerRecordError> {
        if binding != self.binding {
            return Err(DesktopProfileOwnerRecordError::BindingMismatch);
        }
        Ok(self.endpoint.clone())
    }
}

struct DesktopRepeatedLaunchFixture {
    directory: TempDir,
    security: DesktopProfileSecurity,
    profile: DesktopProfileId,
}

impl DesktopRepeatedLaunchFixture {
    fn new() -> DesktopRepeatedLaunchResult<Self> {
        Ok(Self {
            directory: TempDir::new()?,
            security: DesktopProfileSecurity::current()?,
            profile: DesktopProfileId::from_str("d92f840f-e575-4654-92bd-79b3ba8b8e55")?,
        })
    }

    fn data(&self) -> DesktopRepeatedLaunchResult<PathBuf> {
        Ok(self.directory.path().canonicalize()?.join("profile"))
    }

    fn admit(&self) -> DesktopRepeatedLaunchResult<DesktopProfileEnumAdmission> {
        Ok(
            DesktopProfileAdmission::new(self.security.clone(), DesktopProfileOsLock)
                .acquire(&self.data()?, self.profile)?,
        )
    }

    fn owner(&self) -> DesktopRepeatedLaunchResult<Box<DesktopProfileLease>> {
        match self.admit()? {
            DesktopProfileEnumAdmission::Owned(lease) => Ok(lease),
            _ => Err("expected the launch to own the profile".into()),
        }
    }

    fn assert_busy(&self) -> DesktopRepeatedLaunchResult {
        assert!(matches!(self.admit()?, DesktopProfileEnumAdmission::Busy));
        Ok(())
    }

    fn binding(
        lease: &DesktopProfileLease,
    ) -> DesktopRepeatedLaunchResult<DesktopProfileActivationBinding> {
        Ok(DesktopProfileActivationBinding::new(
            lease.identity().profile_id(),
            DesktopRequestId::from_str("191b8766-233c-4a15-820a-65f3e042b730")?,
        ))
    }

    fn pid() -> DesktopRepeatedLaunchResult<NonZeroU32> {
        NonZeroU32::new(std::process::id()).ok_or_else(|| "missing process id".into())
    }

    fn phase(&self) -> DesktopRepeatedLaunchResult<DesktopProfileEnumPhase> {
        Ok(
            DesktopProfileOwnerRecord::read(&self.data()?, &self.security)?
                .ok_or("missing owner record")?
                .phase(),
        )
    }
}

#[test]
fn canceled_quit_restores_each_exact_phase_and_a_second_launch_stays_a_non_writer()
-> DesktopRepeatedLaunchResult {
    for resting in [
        DesktopProfileEnumPhase::Ready,
        DesktopProfileEnumPhase::Recovery,
        DesktopProfileEnumPhase::InstallPending,
    ] {
        let fixture = DesktopRepeatedLaunchFixture::new()?;
        let lease = fixture.owner()?;
        let binding = DesktopRepeatedLaunchFixture::binding(&lease)?;
        let proof = DesktopRepeatedLaunchEndpoint::new(binding)?;
        let mut publisher = DesktopProfileOwnerRecordPublisher::publish(
            &lease,
            binding,
            &proof,
            DesktopRepeatedLaunchFixture::pid()?,
        )?;
        fixture.assert_busy()?;
        publisher.transition(resting)?;
        assert_eq!(fixture.phase()?, resting);
        fixture.assert_busy()?;
        publisher.transition(DesktopProfileEnumPhase::ShuttingDown)?;
        assert_eq!(fixture.phase()?, DesktopProfileEnumPhase::ShuttingDown);
        fixture.assert_busy()?;
        publisher.transition(resting)?;
        assert_eq!(fixture.phase()?, resting);
        fixture.assert_busy()?;
        lease.verify_identity()?;
    }
    Ok(())
}

#[test]
fn confirmed_exit_releases_the_lease_and_a_stale_record_never_blocks_the_next_owner()
-> DesktopRepeatedLaunchResult {
    let fixture = DesktopRepeatedLaunchFixture::new()?;
    let lease = fixture.owner()?;
    let binding = DesktopRepeatedLaunchFixture::binding(&lease)?;
    let proof = DesktopRepeatedLaunchEndpoint::new(binding)?;
    let mut publisher = DesktopProfileOwnerRecordPublisher::publish(
        &lease,
        binding,
        &proof,
        DesktopRepeatedLaunchFixture::pid()?,
    )?;
    publisher.transition(DesktopProfileEnumPhase::Ready)?;
    publisher.transition(DesktopProfileEnumPhase::ShuttingDown)?;
    fixture.assert_busy()?;
    drop(publisher);
    fixture.assert_busy()?;
    drop(lease);
    assert_eq!(fixture.phase()?, DesktopProfileEnumPhase::ShuttingDown);
    let next = fixture.owner()?;
    next.verify_identity()?;
    fixture.assert_busy()?;
    Ok(())
}

#[tokio::test]
async fn activation_requests_the_window_but_leaves_a_recovered_paused_game_untouched()
-> DesktopRepeatedLaunchResult {
    let fixture = DesktopRepeatedLaunchFixture::new()?;
    let lease = fixture.owner()?;
    let storage = DesktopGameSupervisorFixtureStorage::new(2).await?;
    let repository = StorageLibraryPlayedRepository::new(storage.store.client(), &storage.executor);
    let mut session = GameTransitionFixture::session(ClockTimeControl::initial_default(), None)?;
    GameTransitionFixture::play(&mut session, 2, "e2e4", 1_000)?;
    let saved = repository
        .save(GameStorageSaveRequest::capture_with_change(
            &session,
            ClockInstant::from_millis(2_000)?,
            GameStorageRevision::INITIAL,
            GameStorageEnumContentChange::Changed(GameStorageContentEdit::new(
                session.revision(),
                session.revision(),
                GameStorageEnumEditCause::Created,
                GameLibraryOrganizationTimestamp::new(10)?,
            )?),
        )?)
        .await?;
    let recovered = repository
        .load(session.id())
        .await?
        .ok_or("missing saved game")?;
    assert_eq!(recovered.session().phase(), GameEnumPhase::Interrupted);
    assert!(!recovered.session().clock().is_running());
    assert_eq!(recovered.session().revision(), session.revision());
    assert_eq!(recovered.storage_revision(), saved.storage_revision());

    fixture.assert_busy()?;
    let wake = DesktopNativeEventLoopWake::new();
    assert!(!wake.take_activation_request());
    let result = wake.request_activation()?;
    assert_eq!(result, DesktopProfileActivationEnumResult::OwnerStarting);
    assert!(wake.take_activation_request());
    assert!(!wake.take_activation_request());

    let after = repository
        .load(session.id())
        .await?
        .ok_or("missing saved game after activation")?;
    assert_eq!(after.session().phase(), GameEnumPhase::Interrupted);
    assert!(!after.session().clock().is_running());
    assert_eq!(after.session().revision(), recovered.session().revision());
    assert_eq!(after.storage_revision(), recovered.storage_revision());
    lease.verify_identity()?;
    drop((repository, recovered, after));
    storage.close().await
}
