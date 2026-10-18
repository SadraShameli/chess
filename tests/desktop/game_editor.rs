use std::time::Duration;

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::board::position::BoardPosition;
use crate::board::start::BoardStart;
use crate::clock::duration::{ClockDuration, ClockInstant};
use crate::clock::state::ClockState;
use crate::clock::time_control::ClockTimeControl;
use crate::desktop::game_editor::{
    DesktopGameEditor, DesktopGameEditorBinding, DesktopGameEditorEnumSource,
    DesktopGameEditorEpoch, DesktopGameEditorError, DesktopGameEditorGeneration,
    DesktopGameEditorSnapshotEnumState, DesktopGameEditorTreePage,
};
use crate::desktop::identity::DesktopRequestId;
use crate::game::annotations::GameAnnotationsEnumVariationRequest;
use crate::game::enums::{
    GameEnumEngineReadiness, GameEnumMarkingColor, GameEnumMode, GameEnumPhase, GameEnumResult,
    GameEnumSeat, GameId, GameMoveId, GameRevision,
};
use crate::game::mate_proof::{GameMateProof, GameMateProofBudget, GameMateProofControl};
use crate::game::move_node::{GameMoveNodeAnnotations, GameMoveNodeMarking, GameMoveNodeNag};
use crate::game::session::{
    GameMetadata, GameMetadataKnownTags, GameMetadataText, GameSeats, GameSession,
    GameSessionMoveRequest, GameSessionOptions,
};
use crate::game::storage_repository::GameStorageRevision;
use crate::game::tree::GameTree;
use crate::notation::game::{NotationGame, NotationGameClock, NotationGameParts};
use crate::notation::game_identity::NotationGameIdentityEnumResult;
use crate::storage::game_document::StorageGameDocument;
use crate::storage::library_record::{StorageLibraryEnumContent, StorageLibraryRecord};

struct DesktopGameEditorFixture;

struct DesktopGameEditorFixtureControl;

impl GameMateProofControl for DesktopGameEditorFixtureControl {
    fn now(&self) -> Duration {
        Duration::ZERO
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}

impl DesktopGameEditorFixture {
    fn node(value: u128) -> Result<GameMoveId, Box<dyn std::error::Error>> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn game(value: u128) -> Result<GameId, Box<dyn std::error::Error>> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn epoch(value: u128) -> Result<DesktopGameEditorEpoch, Box<dyn std::error::Error>> {
        let request: DesktopRequestId = uuid::Uuid::from_u128(value).to_string().parse()?;
        Ok(DesktopGameEditorEpoch::from_request_id(request))
    }

    fn binding() -> Result<DesktopGameEditorBinding, Box<dyn std::error::Error>> {
        Ok(DesktopGameEditorBinding::new(
            GameStorageRevision::new(7),
            Self::epoch(1000)?,
            DesktopGameEditorGeneration::ZERO,
        ))
    }

    fn metadata() -> Result<GameMetadata, Box<dyn std::error::Error>> {
        Ok(GameMetadata::new(
            GameMetadataKnownTags {
                white: Some(GameMetadataText::new("Original player".to_owned())?),
                ..GameMetadataKnownTags::default()
            },
            Vec::new(),
        )?)
    }

    fn annotations() -> Result<GameMoveNodeAnnotations, Box<dyn std::error::Error>> {
        Ok(GameMoveNodeAnnotations::new(
            "original plain-text notes <b>remain text</b>".to_owned(),
            vec![GameMoveNodeNag::new(1), GameMoveNodeNag::new(14)],
            vec![GameMoveNodeMarking::arrow(
                GameEnumMarkingColor::Green,
                "g1".parse()?,
                "f3".parse()?,
            )?],
        )?)
    }

    fn notation(plies: u128) -> Result<NotationGame, Box<dyn std::error::Error>> {
        let start = BoardStart::standard();
        let root = Self::node(1)?;
        let mut tree = GameTree::new(root, start.position().clone(), None);
        let mut tip = root;
        let cycle = ["g1f3", "g8f6", "f3g1", "f6g8"];
        for ply in 0..plies {
            let index = usize::try_from(ply % 4)?;
            let movement = tree.node(tip)?.position().parse_uci(cycle[index])?;
            tip = tree.append(tip, Self::node(ply + 2)?, movement, None)?;
        }
        tree.set_annotations(root, Self::annotations()?)?;
        for (id, uci) in [(500, "d2d4"), (501, "e2e4"), (502, "c2c4")] {
            let movement = tree.node(root)?.position().parse_uci(uci)?;
            tree.append(root, Self::node(id)?, movement, None)?;
        }
        let mut alternate = root;
        for (id, uci) in [(600, "b1c3"), (601, "b8c6"), (602, "c3b1"), (603, "c6b8")] {
            let movement = tree.node(alternate)?.position().parse_uci(uci)?;
            alternate = tree.append(alternate, Self::node(id)?, movement, None)?;
        }
        let clock = NotationGameClock::new(root, ClockDuration::from_nanos(123));
        Ok(NotationGame::new(NotationGameParts {
            start,
            tree,
            mainline_tip: tip,
            metadata: Self::metadata()?,
            result: NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw),
            result_header: Some(NotationGameIdentityEnumResult::Completed(
                GameEnumResult::Draw,
            )),
            time_control: Some(GameMetadataText::new("300+2".to_owned())?),
            clocks: vec![clock],
            warnings: Vec::new(),
        })?)
    }

    fn record(plies: u128) -> Result<StorageLibraryRecord, Box<dyn std::error::Error>> {
        Ok(StorageLibraryRecord::new(
            Self::game(900)?,
            StorageLibraryEnumContent::Notation(Box::new(Self::notation(plies)?)),
        )?)
    }

    fn played() -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session = GameSession::new(GameSessionOptions {
            id: Self::game(900)?,
            root: Self::node(1)?,
            start: BoardStart::standard(),
            seats: GameSeats::new(
                GameEnumMode::LocalTwoPlayer,
                GameEnumSeat::Human,
                GameEnumSeat::Human,
            )?,
            clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
            metadata: Self::metadata()?,
        })?;
        let proof = GameMateProof::new(
            GameMateProofBudget::default(),
            DesktopGameEditorFixtureControl,
        );
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::NotRequired,
            &proof,
        )?;
        Ok(session)
    }

    fn play(
        session: &mut GameSession,
        id: u128,
        uci: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let position = session.tree().node(session.live())?.position();
        session.accept_move(
            GameSessionMoveRequest {
                expected_revision: session.revision(),
                seat: GameEnumSeat::Human,
                color: position.turn(),
                node: Self::node(id)?,
                movement: position.parse_uci(uci)?,
                received_at: ClockInstant::from_nanos(0),
            },
            &GameMateProof::new(
                GameMateProofBudget::default(),
                DesktopGameEditorFixtureControl,
            ),
        )?;
        Ok(())
    }

    fn completed() -> Result<GameSession, Box<dyn std::error::Error>> {
        let mut session = Self::played()?;
        for (id, movement) in [(2, "f2f3"), (3, "e7e5"), (4, "g2g4"), (5, "d8h4")] {
            Self::play(&mut session, id, movement)?;
        }
        Ok(session)
    }
}

#[test]
fn generation_is_checked_and_epoch_preserves_injected_request_identity()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(DesktopGameEditorGeneration::ZERO.next()?.value(), 1);
    assert!(matches!(
        DesktopGameEditorGeneration::new(u64::MAX).next(),
        Err(DesktopGameEditorError::GenerationExhausted)
    ));
    let request: DesktopRequestId = "00000000-0000-0000-0000-000000000123".parse()?;
    assert_eq!(
        DesktopGameEditorEpoch::from_request_id(request).request_id(),
        request
    );
    assert_eq!(
        DesktopGameEditorEpoch::from_request_id(request),
        DesktopGameEditorFixture::epoch(0x123)?
    );
    Ok(())
}

#[test]
fn notation_constructor_rejects_unknown_selected_node_and_played_record()
-> Result<(), Box<dyn std::error::Error>> {
    let record = DesktopGameEditorFixture::record(4)?;
    assert!(matches!(
        DesktopGameEditor::notation(
            &record,
            GameRevision::ZERO,
            DesktopGameEditorFixture::node(999)?,
            DesktopGameEditorFixture::binding()?
        ),
        Err(DesktopGameEditorError::Tree(_))
    ));
    let played = StorageLibraryRecord::new(
        DesktopGameEditorFixture::game(900)?,
        StorageLibraryEnumContent::Played(Box::new(DesktopGameEditorFixture::played()?)),
    )?;
    assert!(matches!(
        DesktopGameEditor::notation(
            &played,
            GameRevision::ZERO,
            DesktopGameEditorFixture::node(1)?,
            DesktopGameEditorFixture::binding()?
        ),
        Err(DesktopGameEditorError::ExpectedNotation)
    ));
    Ok(())
}

#[test]
fn notation_snapshot_preserves_selected_annotations_metadata_result_and_clock_annotation()
-> Result<(), Box<dyn std::error::Error>> {
    let record = DesktopGameEditorFixture::record(4)?;
    let editor = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO.next()?,
        DesktopGameEditorFixture::node(1)?,
        DesktopGameEditorFixture::binding()?,
    )?;
    let context = editor.context();
    let snapshot = editor.snapshot(context)?;
    assert_eq!(context.game(), record.id());
    assert_eq!(context.storage_revision(), GameStorageRevision::new(7));
    assert_eq!(
        context.source(),
        DesktopGameEditorEnumSource::Notation {
            revision: GameRevision::ZERO.next()?
        }
    );
    assert_eq!(snapshot.context(), context);
    assert_eq!(snapshot.root(), DesktopGameEditorFixture::node(1)?);
    assert_eq!(snapshot.mainline_tip(), DesktopGameEditorFixture::node(5)?);
    assert_eq!(snapshot.metadata(), record.metadata());
    assert_eq!(
        snapshot.annotations(),
        &DesktopGameEditorFixture::annotations()?
    );
    assert_eq!(snapshot.board().fen(), BoardPosition::standard().fen());
    match snapshot.state() {
        DesktopGameEditorSnapshotEnumState::Notation {
            result,
            result_header,
            time_control,
            selected_clock_annotation,
        } => {
            assert_eq!(
                *result,
                NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw)
            );
            assert_eq!(*result_header, Some(*result));
            assert_eq!(
                time_control.as_ref().map(GameMetadataText::as_str),
                Some("300+2")
            );
            assert_eq!(
                *selected_clock_annotation,
                Some(ClockDuration::from_nanos(123))
            );
        }
        DesktopGameEditorSnapshotEnumState::Played { .. } => {
            return Err("Notation acquired played state".into());
        }
    }
    Ok(())
}

#[test]
fn selected_board_uses_canonical_native_moves_and_declared_variant()
-> Result<(), Box<dyn std::error::Error>> {
    for (fen, variant) in [
        ("7k/P7/8/8/8/8/8/7K w - - 0 1", BoardEnumVariant::Standard),
        ("4k3/8/8/8/8/8/8/5K1R w H - 0 1", BoardEnumVariant::Chess960),
    ] {
        let position = BoardPosition::from_fen(fen, variant)?;
        let root = DesktopGameEditorFixture::node(1)?;
        let game = NotationGame::new(NotationGameParts {
            start: BoardStart::custom(position.clone()),
            tree: GameTree::new(root, position.clone(), None),
            mainline_tip: root,
            metadata: GameMetadata::default(),
            result: NotationGameIdentityEnumResult::Unfinished,
            result_header: None,
            time_control: None,
            clocks: Vec::new(),
            warnings: Vec::new(),
        })?;
        let record = StorageLibraryRecord::new(
            DesktopGameEditorFixture::game(900)?,
            StorageLibraryEnumContent::Notation(Box::new(game)),
        )?;
        let editor = DesktopGameEditor::notation(
            &record,
            GameRevision::ZERO,
            root,
            DesktopGameEditorFixture::binding()?,
        )?;
        let snapshot = editor.snapshot(editor.context())?;
        assert_eq!(snapshot.board().variant(), variant);
        assert_eq!(snapshot.board().fen(), position.fen());
        assert_eq!(
            snapshot
                .board()
                .legal_moves()
                .iter()
                .map(|movement| movement.movement())
                .collect::<Vec<_>>(),
            position.legal_moves()
        );
        assert!(matches!(
            snapshot.state(),
            DesktopGameEditorSnapshotEnumState::Notation {
                result: NotationGameIdentityEnumResult::Unfinished,
                result_header: None,
                time_control: None,
                selected_clock_annotation: None
            }
        ));
    }
    Ok(())
}

#[test]
fn full_context_rejects_source_game_storage_epoch_generation_revision_and_view_changes()
-> Result<(), Box<dyn std::error::Error>> {
    let record = DesktopGameEditorFixture::record(4)?;
    let root = DesktopGameEditorFixture::node(1)?;
    let binding = DesktopGameEditorFixture::binding()?;
    let session = DesktopGameEditorFixture::played()?;
    let revision = session.revision();
    let editor = DesktopGameEditor::notation(&record, revision, root, binding)?;
    let expected = editor.context();
    let board = editor.snapshot(expected)?.board().fen().to_owned();
    let other_record = StorageLibraryRecord::new(
        DesktopGameEditorFixture::game(901)?,
        StorageLibraryEnumContent::Notation(Box::new(DesktopGameEditorFixture::notation(4)?)),
    )?;
    let alternatives = [
        DesktopGameEditor::played(&session, binding)?,
        DesktopGameEditor::notation(&other_record, revision, root, binding)?,
        DesktopGameEditor::notation(
            &record,
            revision,
            root,
            DesktopGameEditorBinding::new(
                GameStorageRevision::new(8),
                binding.epoch(),
                binding.generation(),
            ),
        )?,
        DesktopGameEditor::notation(
            &record,
            revision,
            root,
            DesktopGameEditorBinding::new(
                binding.storage_revision(),
                DesktopGameEditorFixture::epoch(1001)?,
                binding.generation(),
            ),
        )?,
        DesktopGameEditor::notation(
            &record,
            revision,
            root,
            DesktopGameEditorBinding::new(
                binding.storage_revision(),
                binding.epoch(),
                binding.generation().next()?,
            ),
        )?,
        DesktopGameEditor::notation(&record, revision.next()?, root, binding)?,
    ];
    for alternative in alternatives {
        assert_eq!(
            alternative.snapshot(alternative.context())?.board().fen(),
            board
        );
        assert!(matches!(
            alternative.context().check_expected(expected),
            Err(DesktopGameEditorError::StaleContext)
        ));
        assert!(matches!(
            alternative.snapshot(expected),
            Err(DesktopGameEditorError::StaleContext)
        ));
        assert!(matches!(
            alternative.tree_page(expected, None),
            Err(DesktopGameEditorError::StaleContext)
        ));
        assert!(matches!(
            alternative.children_page(expected, root, None),
            Err(DesktopGameEditorError::StaleContext)
        ));
    }
    let other_view = DesktopGameEditor::notation(
        &record,
        revision,
        DesktopGameEditorFixture::node(5)?,
        binding,
    )?;
    assert_eq!(
        other_view
            .snapshot(other_view.context())?
            .board()
            .pieces()
            .len(),
        editor.snapshot(expected)?.board().pieces().len()
    );
    assert_eq!(
        other_view.snapshot(other_view.context())?.board().turn(),
        editor.snapshot(expected)?.board().turn()
    );
    assert!(matches!(
        other_view.snapshot(expected),
        Err(DesktopGameEditorError::StaleContext)
    ));
    Ok(())
}

#[test]
fn distinct_selected_nodes_with_identical_fen_do_not_share_context()
-> Result<(), Box<dyn std::error::Error>> {
    let record = DesktopGameEditorFixture::record(4)?;
    let binding = DesktopGameEditorFixture::binding()?;
    let first = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO,
        DesktopGameEditorFixture::node(5)?,
        binding,
    )?;
    let second = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO,
        DesktopGameEditorFixture::node(603)?,
        binding,
    )?;
    assert_eq!(
        first.snapshot(first.context())?.board().fen(),
        second.snapshot(second.context())?.board().fen()
    );
    assert!(matches!(
        second.snapshot(first.context()),
        Err(DesktopGameEditorError::StaleContext)
    ));
    assert!(matches!(
        second.children_page(first.context(), DesktopGameEditorFixture::node(1)?, None),
        Err(DesktopGameEditorError::StaleContext)
    ));
    Ok(())
}

#[test]
fn played_context_uses_real_identity_revision_view_and_live_tip()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameEditorFixture::played()?;
    DesktopGameEditorFixture::play(&mut session, 2, "e2e4")?;
    session.select_view(DesktopGameEditorFixture::node(1)?)?;
    let editor = DesktopGameEditor::played(&session, DesktopGameEditorFixture::binding()?)?;
    let context = editor.context();
    let snapshot = editor.snapshot(context)?;
    assert_eq!(context.game(), session.id());
    assert_eq!(
        context.source(),
        DesktopGameEditorEnumSource::Played {
            revision: session.revision()
        }
    );
    assert_eq!(context.selected(), session.view());
    assert_eq!(
        snapshot.board().fen(),
        session.tree().node(session.view())?.position().fen()
    );
    assert_eq!(snapshot.mainline_tip(), session.study_mainline_tip());
    assert!(
        matches!(snapshot.state(), DesktopGameEditorSnapshotEnumState::Played { live, phase: GameEnumPhase::Active, outcome: None } if *live == session.live())
    );
    assert!(session.clock().is_running());
    Ok(())
}

#[test]
fn played_completed_result_and_independent_study_tip_are_preserved()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameEditorFixture::completed()?;
    let outcome = session.outcome().ok_or("Missing played outcome")?;
    session.select_view(DesktopGameEditorFixture::node(1)?)?;
    let movement = session
        .tree()
        .node(session.view())?
        .position()
        .parse_uci("d2d4")?;
    let branch = session
        .append_study_variation(
            session.revision(),
            DesktopGameEditorFixture::node(500)?,
            movement,
        )?
        .view();
    let confirmation = session
        .preview_variation(
            session.revision(),
            GameAnnotationsEnumVariationRequest::Promote { tip: branch },
        )?
        .confirm();
    session.apply_variation(confirmation)?;
    session.save_notes_and_nags(
        session.revision(),
        branch,
        "played branch notes".to_owned(),
        vec![GameMoveNodeNag::new(1)],
    )?;
    let editor = DesktopGameEditor::played(&session, DesktopGameEditorFixture::binding()?)?;
    let snapshot = editor.snapshot(editor.context())?;
    assert!(
        matches!(snapshot.state(), DesktopGameEditorSnapshotEnumState::Played { live, phase: GameEnumPhase::Completed, outcome: Some(actual) } if *live == session.live() && *actual == outcome)
    );
    assert_eq!(snapshot.mainline_tip(), session.study_mainline_tip());
    assert_eq!(snapshot.mainline_tip(), branch);
    assert_ne!(snapshot.mainline_tip(), session.live());
    assert_eq!(
        snapshot.annotations(),
        session.tree().node(branch)?.annotations()
    );
    assert_eq!(snapshot.context().selected(), branch);
    assert_ne!(branch, session.live());
    Ok(())
}

#[test]
fn insertion_pages_cover_more_than_one_hundred_nodes_with_repeated_determinism()
-> Result<(), Box<dyn std::error::Error>> {
    let record = DesktopGameEditorFixture::record(120)?;
    let StorageLibraryEnumContent::Notation(game) = record.content() else {
        return Err("Missing notation".into());
    };
    let expected: Vec<_> = game.tree().nodes().map(|node| node.id()).collect();
    let editor = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO,
        game.tree().root(),
        DesktopGameEditorFixture::binding()?,
    )?;
    let context = editor.context();
    let first = editor.tree_page(context, None)?;
    assert_eq!(first.context(), context);
    assert_eq!(first, editor.tree_page(context, None)?);
    assert_eq!(first.nodes().len(), DesktopGameEditorTreePage::MAX_ROWS);
    let cursor = first.next_cursor().ok_or("Missing second page")?;
    let second = editor.tree_page(context, Some(cursor))?;
    assert_eq!(second.context(), context);
    assert_eq!(second, editor.tree_page(context, Some(cursor))?);
    assert_eq!(second.next_cursor(), None);
    let actual: Vec<_> = first
        .nodes()
        .iter()
        .chain(second.nodes())
        .map(|node| node.id())
        .collect();
    assert_eq!(actual, expected);
    assert!(second.nodes().len() <= DesktopGameEditorTreePage::MAX_ROWS);
    for row in first.nodes().iter().chain(second.nodes()) {
        let canonical = game.tree().node(row.id())?;
        assert_eq!(row.parent(), canonical.parent());
        assert_eq!(row.san(), canonical.san());
        assert_eq!(row.child_count(), canonical.children().len());
        assert_eq!(row.first_child(), canonical.children().first().copied());
        assert_eq!(row.has_notes(), !canonical.annotations().notes().is_empty());
        assert_eq!(row.has_nags(), !canonical.annotations().nags().is_empty());
        assert_eq!(
            row.has_markings(),
            !canonical.annotations().markings().is_empty()
        );
    }
    Ok(())
}

#[test]
fn children_pages_follow_promoted_canonical_order_and_retain_original_mainline()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = DesktopGameEditorFixture::notation(4)?;
    let root = game.tree().root();
    let original_tip = game.mainline_tip();
    let promoted = DesktopGameEditorFixture::node(500)?;
    let confirmation = game
        .preview_variation(GameAnnotationsEnumVariationRequest::Promote { tip: promoted })?
        .confirm();
    game.apply_variation(confirmation, root)?;
    let expected = game.tree().node(root)?.children().to_vec();
    let record = StorageLibraryRecord::new(
        DesktopGameEditorFixture::game(900)?,
        StorageLibraryEnumContent::Notation(Box::new(game)),
    )?;
    let editor = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO,
        root,
        DesktopGameEditorFixture::binding()?,
    )?;
    let page = editor.children_page(editor.context(), root, None)?;
    assert_eq!(page.context(), editor.context());
    assert_eq!(page.parent(), root);
    assert_eq!(
        page.nodes()
            .iter()
            .map(|node| node.id())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(page.nodes().first().map(|node| node.id()), Some(promoted));
    assert_eq!(page.next_cursor(), None);
    let tree = editor.tree_page(editor.context(), None)?;
    assert!(tree.nodes().iter().any(|node| node.id() == original_tip));
    assert_eq!(editor.snapshot(editor.context())?.mainline_tip(), promoted);
    assert_eq!(
        editor.snapshot(editor.context())?.annotations(),
        &DesktopGameEditorFixture::annotations()?
    );
    Ok(())
}

#[test]
fn wide_parent_children_are_paged_with_a_hard_bound_and_canonical_order()
-> Result<(), Box<dyn std::error::Error>> {
    let position = BoardPosition::from_fen(
        "R6R/3Q2Q1/1Q4Q1/4Q3/2Q4Q/Q4Q2/pp6/kBNN1KB1 w - - 0 1",
        BoardEnumVariant::Standard,
    )?;
    let root = DesktopGameEditorFixture::node(1)?;
    let mut tree = GameTree::new(root, position.clone(), None);
    let movements = position.legal_moves();
    assert!(movements.len() > DesktopGameEditorTreePage::MAX_ROWS);
    for (index, movement) in movements.into_iter().enumerate() {
        tree.append(
            root,
            DesktopGameEditorFixture::node(u128::try_from(index)? + 2)?,
            movement,
            None,
        )?;
    }
    let expected = tree.node(root)?.children().to_vec();
    let game = NotationGame::new(NotationGameParts {
        start: BoardStart::custom(position),
        tree,
        mainline_tip: expected[0],
        metadata: GameMetadata::default(),
        result: NotationGameIdentityEnumResult::Unfinished,
        result_header: None,
        time_control: None,
        clocks: Vec::new(),
        warnings: Vec::new(),
    })?;
    let record = StorageLibraryRecord::new(
        DesktopGameEditorFixture::game(900)?,
        StorageLibraryEnumContent::Notation(Box::new(game)),
    )?;
    let editor = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO,
        root,
        DesktopGameEditorFixture::binding()?,
    )?;
    let tree_cursor = editor
        .tree_page(editor.context(), None)?
        .next_cursor()
        .ok_or("Missing tree cursor")?;
    let children_cursor = editor
        .children_page(editor.context(), root, None)?
        .next_cursor()
        .ok_or("Missing children cursor")?;
    let binding = DesktopGameEditorFixture::binding()?;
    for changed in [
        DesktopGameEditor::notation(&record, GameRevision::ZERO.next()?, root, binding)?,
        DesktopGameEditor::notation(
            &record,
            GameRevision::ZERO,
            root,
            DesktopGameEditorBinding::new(
                GameStorageRevision::new(8),
                binding.epoch(),
                binding.generation(),
            ),
        )?,
        DesktopGameEditor::notation(
            &record,
            GameRevision::ZERO,
            root,
            DesktopGameEditorBinding::new(
                binding.storage_revision(),
                DesktopGameEditorFixture::epoch(1001)?,
                binding.generation(),
            ),
        )?,
        DesktopGameEditor::notation(
            &record,
            GameRevision::ZERO,
            root,
            DesktopGameEditorBinding::new(
                binding.storage_revision(),
                binding.epoch(),
                binding.generation().next()?,
            ),
        )?,
        DesktopGameEditor::notation(&record, GameRevision::ZERO, expected[0], binding)?,
    ] {
        assert!(matches!(
            changed.tree_page(changed.context(), Some(tree_cursor)),
            Err(DesktopGameEditorError::StaleContext)
        ));
        assert!(matches!(
            changed.children_page(changed.context(), root, Some(children_cursor)),
            Err(DesktopGameEditorError::StaleContext)
        ));
    }
    assert!(matches!(
        editor.children_page(editor.context(), expected[0], Some(children_cursor)),
        Err(DesktopGameEditorError::InvalidCursor)
    ));
    let mut cursor = None;
    let mut actual = Vec::new();
    loop {
        let page = editor.children_page(editor.context(), root, cursor)?;
        assert_eq!(page, editor.children_page(editor.context(), root, cursor)?);
        assert!(page.nodes().len() <= DesktopGameEditorTreePage::MAX_ROWS);
        actual.extend(page.nodes().iter().map(|node| node.id()));
        let Some(next) = page.next_cursor() else {
            break;
        };
        cursor = Some(next);
    }
    assert_eq!(actual, expected);
    let context = editor.context();
    let StorageLibraryEnumContent::Notation(mut game) = record.into_content() else {
        return Err("Missing notation".into());
    };
    let last = *expected.last().ok_or("Missing last child")?;
    let confirmation = game
        .preview_variation(GameAnnotationsEnumVariationRequest::Promote { tip: last })?
        .confirm();
    game.apply_variation(confirmation, root)?;
    let reordered = StorageLibraryRecord::new(
        DesktopGameEditorFixture::game(900)?,
        StorageLibraryEnumContent::Notation(game),
    )?;
    let reordered_editor =
        DesktopGameEditor::notation(&reordered, GameRevision::ZERO, root, binding)?;
    assert_eq!(context, reordered_editor.context());
    assert!(matches!(
        reordered_editor.children_page(context, root, Some(children_cursor)),
        Err(DesktopGameEditorError::InvalidCursor)
    ));
    Ok(())
}

#[test]
fn tree_cursor_refuses_old_binding_and_wrong_canonical_position()
-> Result<(), Box<dyn std::error::Error>> {
    let record = DesktopGameEditorFixture::record(120)?;
    let root = DesktopGameEditorFixture::node(1)?;
    let binding = DesktopGameEditorFixture::binding()?;
    let editor = DesktopGameEditor::notation(&record, GameRevision::ZERO, root, binding)?;
    let cursor = editor
        .tree_page(editor.context(), None)?
        .next_cursor()
        .ok_or("Missing cursor")?;
    let current = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO,
        root,
        DesktopGameEditorBinding::new(
            binding.storage_revision(),
            DesktopGameEditorFixture::epoch(1001)?,
            binding.generation(),
        ),
    )?;
    assert!(matches!(
        current.tree_page(current.context(), Some(cursor)),
        Err(DesktopGameEditorError::StaleContext)
    ));
    assert!(matches!(
        editor.children_page(editor.context(), DesktopGameEditorFixture::node(999)?, None),
        Err(DesktopGameEditorError::Tree(_))
    ));
    let leaf =
        editor.children_page(editor.context(), DesktopGameEditorFixture::node(121)?, None)?;
    assert!(leaf.nodes().is_empty());
    assert_eq!(leaf.next_cursor(), None);
    let shorter = DesktopGameEditorFixture::record(4)?;
    let shorter_editor = DesktopGameEditor::notation(&shorter, GameRevision::ZERO, root, binding)?;
    assert_eq!(editor.context(), shorter_editor.context());
    assert!(matches!(
        shorter_editor.tree_page(shorter_editor.context(), Some(cursor)),
        Err(DesktopGameEditorError::InvalidCursor)
    ));
    Ok(())
}

#[test]
fn owned_snapshot_remains_unchanged_after_the_source_owner_accepts_annotations()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = DesktopGameEditorFixture::completed()?;
    let original = session.restoration(ClockInstant::from_nanos(0))?;
    let snapshot = {
        let editor = DesktopGameEditor::played(&session, DesktopGameEditorFixture::binding()?)?;
        let first = editor.snapshot(editor.context())?;
        editor.tree_page(editor.context(), None)?;
        editor.children_page(editor.context(), session.tree().root(), None)?;
        first
    };
    assert_eq!(session.revision(), original.revision);
    assert_eq!(session.view(), original.view);
    assert_eq!(
        session.clock().snapshot(ClockInstant::from_nanos(0))?,
        original.clock
    );
    session.save_notes_and_nags(
        session.revision(),
        session.view(),
        "new note".to_owned(),
        vec![GameMoveNodeNag::new(2)],
    )?;
    assert_eq!(snapshot.annotations(), &GameMoveNodeAnnotations::default());
    assert_eq!(snapshot.metadata(), &original.session.metadata);
    assert_eq!(
        snapshot.context().source(),
        DesktopGameEditorEnumSource::Played {
            revision: original.revision
        }
    );
    assert_ne!(session.revision(), original.revision);
    Ok(())
}

#[test]
fn notation_projection_never_changes_source_content_or_mainline()
-> Result<(), Box<dyn std::error::Error>> {
    let record = DesktopGameEditorFixture::record(120)?;
    let StorageLibraryEnumContent::Notation(game) = record.content() else {
        return Err("Missing notation".into());
    };
    let before = StorageGameDocument::capture_notation(record.id(), game)?.json()?;
    let editor = DesktopGameEditor::notation(
        &record,
        GameRevision::ZERO,
        game.tree().root(),
        DesktopGameEditorFixture::binding()?,
    )?;
    let snapshot = editor.snapshot(editor.context())?;
    editor.tree_page(editor.context(), None)?;
    editor.children_page(editor.context(), game.tree().root(), None)?;
    assert_eq!(snapshot.mainline_tip(), game.mainline_tip());
    assert_eq!(
        StorageGameDocument::capture_notation(record.id(), game)?.json()?,
        before
    );
    Ok(())
}
