use chess::clock::duration::ClockInstant;
use chess::game::session::{GameSession, GameSessionEnumState};
use chess::storage::library_record::{
    StorageLibraryEnumContent, StorageLibraryRecord, StorageLibraryRecordError,
};

use crate::storage_game_document::{StorageGameDocumentFixture, StorageGameDocumentTestResult};

#[test]
fn played_content_rejects_an_external_identity_different_from_the_session()
-> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::annotated()?;
    assert!(matches!(
        StorageLibraryRecord::new(
            StorageGameDocumentFixture::library_id()?,
            StorageLibraryEnumContent::Played(Box::new(session))
        ),
        Err(StorageLibraryRecordError::Identity { .. })
    ));
    Ok(())
}

#[test]
fn notation_identity_and_length_follow_the_explicit_tip_and_mode_is_unavailable()
-> StorageGameDocumentTestResult {
    let notation = StorageGameDocumentFixture::notation()?;
    let expected_start = notation.start().clone();
    let expected_metadata = notation.metadata().clone();
    let expected_result = notation.result();
    let record = StorageLibraryRecord::new(
        StorageGameDocumentFixture::library_id()?,
        StorageLibraryEnumContent::Notation(Box::new(notation)),
    )?;
    assert_eq!(record.mode(), None);
    assert_eq!(record.start(), &expected_start);
    assert_eq!(record.metadata(), &expected_metadata);
    assert_eq!(record.result(), expected_result);
    assert_eq!(record.identity_line_len()?, 2);
    assert_eq!(
        record.identity()?.structural_key().mainline_uci(),
        &["d2d4", "d7d5"]
    );
    assert!(matches!(
        record.content(),
        StorageLibraryEnumContent::Notation(_)
    ));
    Ok(())
}

#[test]
fn played_identity_uses_only_actual_live_moves_after_a_study_selection_or_takeback()
-> StorageGameDocumentTestResult {
    let session = StorageGameDocumentFixture::annotated()?;
    let id = session.id();
    let mut saved = session.restoration(ClockInstant::from_millis(2_000)?)?;
    saved.view = saved.session.root;
    saved.study_mainline_tip = StorageGameDocumentFixture::node(3)?;
    let record = StorageLibraryRecord::new(
        id,
        StorageLibraryEnumContent::Played(Box::new(GameSession::restore(saved.clone())?)),
    )?;
    let before = record.identity()?;
    assert_eq!(record.identity_line_len()?, 1);
    saved.live = saved.session.root;
    saved.clock = saved.root_clock.clone().ok_or("missing initial clock")?;
    saved.state = GameSessionEnumState::Analysis(None);
    let record = StorageLibraryRecord::new(
        id,
        StorageLibraryEnumContent::Played(Box::new(GameSession::restore(saved)?)),
    )?;
    assert_eq!(record.identity_line_len()?, 0);
    assert!(
        record
            .identity()?
            .structural_key()
            .mainline_uci()
            .is_empty()
    );
    let StorageLibraryEnumContent::Played(session) = record.into_content() else {
        return Err("expected played content".into());
    };
    assert_ne!(session.live(), session.study_mainline_tip());
    assert_eq!(before.structural_key().mainline_uci(), &["e2e4"]);
    assert_eq!(session.tree().len(), 3);
    Ok(())
}
