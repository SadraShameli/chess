use chess::board::editor::{
    BoardEditorDraft, BoardEditorDraftError, BoardEditorDraftId, BoardEditorDraftRevision,
    BoardEditorSession, BoardEditorSessionEnumConfirmation, BoardEditorSessionEnumEdit,
    BoardEditorSessionEnumOperation, BoardEditorSessionEnumOutcome,
    BoardEditorSessionEnumSelection, BoardEditorSessionEnumSetup, BoardEditorSessionError,
    BoardEditorSessionRequest, BoardEditorValidationEnumFinding, BoardEditorValidationEnumStatus,
};
use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;
use chess::board::start::{BoardStart, BoardStartIndex};
use chess::clock::time_control::ClockTimeControl;

use crate::game_transition_fixture::GameTransitionFixture;

#[test]
fn local_editor_history_has_a_finite_retention_bound_and_preserves_recent_edits()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::session()?;
    for fullmoves in 2..=301 {
        BoardEditorFixture::edit(
            &mut session,
            BoardEditorSessionEnumEdit::Counters {
                halfmoves: 0,
                fullmoves,
            },
        )?;
    }
    let mut undo_count = 0;
    while session.has_undo() {
        BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Undo)?;
        undo_count += 1;
    }
    assert_eq!(
        undo_count, 256,
        "editor history retained every prior draft without its finite memory bound"
    );
    assert_eq!(session.draft().fields().fullmoves(), 45);
    for _ in 0..undo_count {
        BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Redo)?;
    }
    assert_eq!(session.draft().fields().fullmoves(), 301);
    assert!(!session.has_redo());
    Ok(())
}

struct BoardEditorFixture;

impl BoardEditorFixture {
    fn id() -> Result<BoardEditorDraftId, Box<dyn std::error::Error>> {
        Ok("123e4567-e89b-42d3-a456-426614174146".parse()?)
    }

    fn session() -> Result<BoardEditorSession, Box<dyn std::error::Error>> {
        Ok(BoardEditorSession::new(Self::id()?, None))
    }

    fn sparse() -> Result<BoardEditorSession, Box<dyn std::error::Error>> {
        let position =
            BoardPosition::from_fen("7k/8/8/8/8/8/8/4K3 w - - 0 1", BoardEnumVariant::Standard)?;
        Ok(BoardEditorSession::new(Self::id()?, Some(&position)))
    }

    fn apply(
        session: &mut BoardEditorSession,
        operation: BoardEditorSessionEnumOperation,
    ) -> Result<BoardEditorSessionEnumOutcome, BoardEditorSessionError> {
        session.apply(BoardEditorSessionRequest {
            draft: session.draft().id(),
            expected_revision: session.draft().revision(),
            operation,
        })
    }

    fn edit(
        session: &mut BoardEditorSession,
        edit: BoardEditorSessionEnumEdit,
    ) -> Result<BoardEditorSessionEnumOutcome, BoardEditorSessionError> {
        Self::apply(session, BoardEditorSessionEnumOperation::Edit(edit))
    }

    fn select(
        session: &mut BoardEditorSession,
        selection: BoardEditorSessionEnumSelection,
    ) -> Result<BoardEditorSessionEnumOutcome, BoardEditorSessionError> {
        Self::apply(session, BoardEditorSessionEnumOperation::Select(selection))
    }

    fn place(
        session: &mut BoardEditorSession,
        square: &str,
        color: BoardEnumColor,
        kind: BoardEnumPieceKind,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Self::edit(
            session,
            BoardEditorSessionEnumEdit::Place {
                square: square.parse()?,
                color,
                kind,
            },
        )?;
        Ok(())
    }
}

#[test]
fn a_new_draft_uses_standard_without_inventing_game_state() -> Result<(), Box<dyn std::error::Error>>
{
    let session = BoardEditorFixture::session()?;
    assert_eq!(session.draft().id(), BoardEditorFixture::id()?);
    assert_eq!(session.draft().revision(), BoardEditorDraftRevision::ZERO);
    assert_eq!(session.draft().validate()?, BoardPosition::standard());
    assert_eq!(
        session.draft().status(),
        BoardEditorValidationEnumStatus::Valid
    );
    assert_eq!(session.selection(), BoardEditorSessionEnumSelection::None);
    assert!(!session.has_undo());
    assert!(!session.has_redo());
    Ok(())
}

#[test]
fn source_copies_preserve_declared_variants_original_rooks_and_en_passant()
-> Result<(), Box<dyn std::error::Error>> {
    let frc = BoardStart::chess960(BoardStartIndex::try_from(0)?)?;
    let dfrc = BoardStart::dfrc(
        BoardStartIndex::try_from(37)?,
        BoardStartIndex::try_from(822)?,
    )?;
    let sources = [
        BoardPosition::standard().play_uci("e2e4")?,
        BoardPosition::from_fen(
            "4k3/8/8/8/8/8/8/RR2K3 w A - 17 42",
            BoardEnumVariant::Chess960,
        )?,
        BoardPosition::from_fen(
            "4k3/8/8/8/8/8/8/RR2K3 w B - 17 42",
            BoardEnumVariant::Chess960,
        )?,
        frc.position().clone(),
        dfrc.position().clone(),
    ];
    for source in sources {
        let session = BoardEditorSession::new(BoardEditorFixture::id()?, Some(&source));
        let restored = session.draft().validate()?;
        assert_eq!(restored, source);
        assert_eq!(restored.identity(), source.identity());
        assert_eq!(session.draft().validated_fen()?, source.fen());
        assert_eq!(session.draft().play_prefill()?.position(), &source);
        let reopened: BoardEditorDraft =
            serde_json::from_str(&serde_json::to_string(session.draft())?)?;
        assert_eq!(reopened.fields(), session.draft().fields());
        assert_eq!(reopened.validated_fen()?, source.fen());
        assert_eq!(
            BoardPosition::from_fen(&session.draft().validated_fen()?, source.variant())?,
            source
        );
    }
    let advanced = BoardPosition::standard().play_uci("e2e4")?;
    let draft = BoardEditorDraft::new(BoardEditorFixture::id()?, Some(&advanced));
    assert_eq!(draft.fields().en_passant(), Some("e3".parse()?));
    Ok(())
}

#[test]
fn editing_a_selected_game_position_leaves_its_tree_and_session_unchanged()
-> Result<(), Box<dyn std::error::Error>> {
    let mut game = GameTransitionFixture::session(ClockTimeControl::untimed(), None)?;
    let root = game.live();
    GameTransitionFixture::play(&mut game, 2, "e2e4", 1_000)?;
    game.select_view(root)?;
    let before_position = game.tree().node(root)?.position().clone();
    let revision = game.revision();
    let live = game.live();
    let view = game.view();
    let tree_size = game.tree().len();
    let clock = game.tree().node(live)?.clock().cloned();
    let mut editor = BoardEditorSession::new(
        BoardEditorFixture::id()?,
        Some(game.tree().node(game.view())?.position()),
    );
    BoardEditorFixture::edit(
        &mut editor,
        BoardEditorSessionEnumEdit::Remove {
            square: "e1".parse()?,
        },
    )?;
    BoardEditorFixture::apply(&mut editor, BoardEditorSessionEnumOperation::Undo)?;
    BoardEditorFixture::apply(&mut editor, BoardEditorSessionEnumOperation::Redo)?;
    assert!(editor.draft().validate().is_err());
    assert_eq!(game.tree().node(root)?.position(), &before_position);
    assert_eq!(game.revision(), revision);
    assert_eq!(game.live(), live);
    assert_eq!(game.view(), view);
    assert_eq!(game.tree().len(), tree_size);
    assert_eq!(game.tree().node(live)?.clock(), clock.as_ref());
    Ok(())
}

#[test]
fn missing_and_extra_kings_remain_editable_with_color_and_count_findings()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::session()?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Remove {
            square: "e1".parse()?,
        },
    )?;
    let validation = session
        .draft()
        .validate()
        .err()
        .ok_or("accepted missing king")?;
    assert!(std::error::Error::source(&validation).is_some());
    assert!(
        validation
            .findings()
            .contains(&BoardEditorValidationEnumFinding::KingCount {
                color: BoardEnumColor::White,
                actual: 0,
            })
    );
    assert!(session.draft().fields().piece_at("e1".parse()?).is_none());
    assert!(session.draft().validated_fen().is_err());
    assert!(session.draft().play_prefill().is_err());
    BoardEditorFixture::place(
        &mut session,
        "e1",
        BoardEnumColor::White,
        BoardEnumPieceKind::King,
    )?;
    BoardEditorFixture::place(
        &mut session,
        "e3",
        BoardEnumColor::White,
        BoardEnumPieceKind::King,
    )?;
    assert!(
        session
            .draft()
            .validate()
            .err()
            .ok_or("accepted two kings")?
            .findings()
            .contains(&BoardEditorValidationEnumFinding::KingCount {
                color: BoardEnumColor::White,
                actual: 2
            })
    );
    Ok(())
}

#[test]
fn adjacent_kings_opposite_check_and_impossible_check_are_canonical_failures()
-> Result<(), Box<dyn std::error::Error>> {
    let mut adjacent = BoardEditorFixture::sparse()?;
    BoardEditorFixture::edit(
        &mut adjacent,
        BoardEditorSessionEnumEdit::Move {
            from: "e1".parse()?,
            to: "g7".parse()?,
        },
    )?;
    assert!(
        adjacent
            .draft()
            .validate()
            .err()
            .ok_or("accepted adjacent kings")?
            .findings()
            .contains(&BoardEditorValidationEnumFinding::OppositeCheck {
                color: BoardEnumColor::Black
            })
    );
    let mut opposite = BoardEditorFixture::sparse()?;
    BoardEditorFixture::place(
        &mut opposite,
        "h1",
        BoardEnumColor::White,
        BoardEnumPieceKind::Rook,
    )?;
    assert!(
        opposite
            .draft()
            .validate()
            .err()
            .ok_or("accepted opposite check")?
            .findings()
            .contains(&BoardEditorValidationEnumFinding::OppositeCheck {
                color: BoardEnumColor::Black
            })
    );
    let mut impossible = BoardEditorFixture::sparse()?;
    for (square, kind) in [
        ("a1", BoardEnumPieceKind::Rook),
        ("e8", BoardEnumPieceKind::Rook),
        ("b4", BoardEnumPieceKind::Bishop),
    ] {
        BoardEditorFixture::place(&mut impossible, square, BoardEnumColor::Black, kind)?;
    }
    assert!(
        impossible
            .draft()
            .validate()
            .err()
            .ok_or("accepted triple check")?
            .findings()
            .contains(&BoardEditorValidationEnumFinding::ImpossibleCheck {
                turn: BoardEnumColor::White
            })
    );
    Ok(())
}

#[test]
fn invalid_rook_rights_and_backrank_pawns_are_retained_for_correction()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::session()?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Remove {
            square: "a1".parse()?,
        },
    )?;
    let error = session
        .draft()
        .validate()
        .err()
        .ok_or("discarded missing rook right")?;
    let original_rook: BoardSquare = "a1".parse()?;
    assert!(error.findings().iter().any(|finding| matches!(finding,
        BoardEditorValidationEnumFinding::CastlingRights { variant: BoardEnumVariant::Standard, rook_squares }
            if rook_squares.contains(&original_rook)
    )));
    assert!(
        session
            .draft()
            .fields()
            .castling_rights()
            .any(|square| square.to_string() == "a1")
    );
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::RevokeCastling("a1".parse()?),
    )?;
    assert!(session.draft().validate().is_ok());
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::GrantCastling("a2".parse()?),
    )?;
    assert!(session.draft().validate().is_err());
    assert!(
        session
            .draft()
            .fields()
            .castling_rights()
            .any(|square| square.to_string() == "a2")
    );
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::RevokeCastling("a2".parse()?),
    )?;
    BoardEditorFixture::place(
        &mut session,
        "a1",
        BoardEnumColor::White,
        BoardEnumPieceKind::Pawn,
    )?;
    assert!(
        session
            .draft()
            .validate()
            .err()
            .ok_or("accepted backrank pawn")?
            .findings()
            .iter()
            .any(|finding| matches!(
                finding,
                BoardEditorValidationEnumFinding::PawnsOnBackrank { .. }
            ))
    );
    Ok(())
}

#[test]
fn en_passant_and_turn_edits_keep_invalid_context_until_corrected()
-> Result<(), Box<dyn std::error::Error>> {
    let source = BoardPosition::from_fen(
        "4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 8",
        BoardEnumVariant::Standard,
    )?;
    let mut session = BoardEditorSession::new(BoardEditorFixture::id()?, Some(&source));
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Turn(BoardEnumColor::Black),
    )?;
    assert_eq!(session.draft().fields().en_passant(), Some("d6".parse()?));
    assert!(
        session
            .draft()
            .validate()
            .err()
            .ok_or("accepted wrong ep turn")?
            .findings()
            .contains(&BoardEditorValidationEnumFinding::EnPassant {
                square: Some("d6".parse()?),
                turn: BoardEnumColor::Black
            })
    );
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::EnPassant(Some("e4".parse()?)),
    )?;
    assert_eq!(session.draft().fields().en_passant(), Some("e4".parse()?));
    assert!(session.draft().validate().is_err());
    BoardEditorFixture::edit(&mut session, BoardEditorSessionEnumEdit::EnPassant(None))?;
    assert_eq!(session.draft().validate()?.turn(), BoardEnumColor::Black);
    Ok(())
}

#[test]
fn counters_accept_exact_bounds_and_retain_signed_out_of_range_values()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::sparse()?;
    for (halfmoves, fullmoves) in [(0, 1), (1_000_000, 1_000_000)] {
        BoardEditorFixture::edit(
            &mut session,
            BoardEditorSessionEnumEdit::Counters {
                halfmoves,
                fullmoves,
            },
        )?;
        let position = session.draft().validate()?;
        assert_eq!(i64::from(position.halfmoves()), halfmoves);
        assert_eq!(i64::from(position.fullmoves()), fullmoves);
        assert_eq!(
            BoardPosition::from_fen(&session.draft().validated_fen()?, position.variant())?,
            position
        );
    }
    for (halfmoves, fullmoves) in [
        (-1, 1),
        (1_000_001, 1),
        (0, 0),
        (0, -1),
        (0, 1_000_001),
        (i64::MAX, i64::MIN),
    ] {
        BoardEditorFixture::edit(
            &mut session,
            BoardEditorSessionEnumEdit::Counters {
                halfmoves,
                fullmoves,
            },
        )?;
        assert_eq!(session.draft().fields().halfmoves(), halfmoves);
        assert_eq!(session.draft().fields().fullmoves(), fullmoves);
        assert_eq!(
            session.draft().status(),
            BoardEditorValidationEnumStatus::Invalid
        );
        assert!(
            !session
                .draft()
                .validate()
                .err()
                .ok_or("accepted invalid counters")?
                .findings()
                .is_empty()
        );
        assert!(session.draft().play_prefill().is_err());
    }
    assert_eq!(
        session
            .draft()
            .validate()
            .err()
            .ok_or("accepted extreme counters")?
            .findings(),
        &[
            BoardEditorValidationEnumFinding::Halfmoves { value: i64::MAX },
            BoardEditorValidationEnumFinding::Fullmoves { value: i64::MIN },
        ]
    );
    Ok(())
}

#[test]
fn changing_variant_does_not_reassign_chess960_rook_rights()
-> Result<(), Box<dyn std::error::Error>> {
    let source = BoardPosition::from_fen(
        "4k3/8/8/8/8/8/8/RR2K3 w B - 0 1",
        BoardEnumVariant::Chess960,
    )?;
    let mut session = BoardEditorSession::new(BoardEditorFixture::id()?, Some(&source));
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Variant(BoardEnumVariant::Standard),
    )?;
    assert!(session.draft().validate().is_err());
    assert_eq!(
        session
            .draft()
            .fields()
            .castling_rights()
            .collect::<Vec<_>>(),
        vec!["b1".parse()?]
    );
    BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Undo)?;
    assert_eq!(session.draft().validate()?, source);
    Ok(())
}

#[test]
fn palette_square_and_direct_edits_support_editor_moves_independent_of_chess_moves()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::session()?;
    BoardEditorFixture::select(
        &mut session,
        BoardEditorSessionEnumSelection::Palette {
            color: BoardEnumColor::Black,
            kind: BoardEnumPieceKind::Knight,
        },
    )?;
    assert!(!session.has_undo());
    assert_eq!(session.draft().revision(), BoardEditorDraftRevision::ZERO);
    BoardEditorFixture::apply(
        &mut session,
        BoardEditorSessionEnumOperation::PlaceSelected("d4".parse()?),
    )?;
    let placed = session
        .draft()
        .fields()
        .piece_at("d4".parse()?)
        .ok_or("missing placed knight")?;
    assert_eq!(placed.color(), BoardEnumColor::Black);
    assert_eq!(placed.kind(), BoardEnumPieceKind::Knight);
    assert_eq!(session.selection(), BoardEditorSessionEnumSelection::None);
    let mut session = BoardEditorFixture::session()?;
    assert!(BoardPosition::standard().play_uci("e2e5").is_err());
    BoardEditorFixture::select(
        &mut session,
        BoardEditorSessionEnumSelection::Square("e2".parse()?),
    )?;
    BoardEditorFixture::apply(
        &mut session,
        BoardEditorSessionEnumOperation::PlaceSelected("e5".parse()?),
    )?;
    assert!(session.draft().fields().piece_at("e2".parse()?).is_none());
    assert_eq!(
        session
            .draft()
            .fields()
            .piece_at("e5".parse()?)
            .ok_or("missing relocated pawn")?
            .kind(),
        BoardEnumPieceKind::Pawn
    );
    assert_eq!(session.draft().fields().turn(), BoardEnumColor::White);
    assert_eq!(session.draft().fields().halfmoves(), 0);
    assert_eq!(session.draft().fields().fullmoves(), 1);
    assert!(session.draft().validate().is_ok());
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Move {
            from: "e5".parse()?,
            to: "e6".parse()?,
        },
    )?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Move {
            from: "e6".parse()?,
            to: "e8".parse()?,
        },
    )?;
    assert!(session.draft().validate().is_err());
    assert_eq!(
        session
            .draft()
            .fields()
            .piece_at("e8".parse()?)
            .ok_or("lost invalid placement")?
            .kind(),
        BoardEnumPieceKind::Pawn
    );
    Ok(())
}

#[test]
fn empty_square_and_absent_palette_errors_are_atomic() -> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::session()?;
    let before = session.draft().clone();
    assert!(matches!(
        BoardEditorFixture::apply(
            &mut session,
            BoardEditorSessionEnumOperation::PlaceSelected("e4".parse()?)
        ),
        Err(BoardEditorSessionError::NoSelection)
    ));
    assert!(matches!(
        BoardEditorFixture::select(
            &mut session,
            BoardEditorSessionEnumSelection::Square("e4".parse()?)
        ),
        Err(BoardEditorSessionError::EmptySquare { .. })
    ));
    assert!(matches!(
        BoardEditorFixture::edit(
            &mut session,
            BoardEditorSessionEnumEdit::Move {
                from: "e4".parse()?,
                to: "d4".parse()?
            }
        ),
        Err(BoardEditorSessionError::EmptySquare { .. })
    ));
    assert_eq!(session.draft(), &before);
    assert!(!session.has_undo());
    Ok(())
}

#[test]
fn clear_board_and_standard_setup_require_confirmation_and_are_undoable()
-> Result<(), Box<dyn std::error::Error>> {
    let source = BoardStart::dfrc(
        BoardStartIndex::try_from(17)?,
        BoardStartIndex::try_from(900)?,
    )?;
    let mut session = BoardEditorSession::new(BoardEditorFixture::id()?, Some(source.position()));
    let before = session.draft().clone();
    for setup in [
        BoardEditorSessionEnumSetup::ClearBoard,
        BoardEditorSessionEnumSetup::StandardSetup,
    ] {
        assert!(matches!(
            BoardEditorFixture::apply(
                &mut session,
                BoardEditorSessionEnumOperation::Replace {
                    setup,
                    confirmation: BoardEditorSessionEnumConfirmation::Required
                }
            ),
            Err(BoardEditorSessionError::ConfirmationRequired { .. })
        ));
        assert_eq!(session.draft(), &before);
    }
    BoardEditorFixture::apply(
        &mut session,
        BoardEditorSessionEnumOperation::Replace {
            setup: BoardEditorSessionEnumSetup::ClearBoard,
            confirmation: BoardEditorSessionEnumConfirmation::Confirmed,
        },
    )?;
    assert_eq!(session.draft().fields().pieces().count(), 0);
    assert_eq!(
        session.draft().fields().variant(),
        BoardEnumVariant::Chess960
    );
    assert_eq!(session.draft().fields().castling_rights().count(), 0);
    assert!(session.draft().validate().is_err());
    BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Undo)?;
    assert_eq!(session.draft().validate()?, *source.position());
    BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Redo)?;
    BoardEditorFixture::apply(
        &mut session,
        BoardEditorSessionEnumOperation::Replace {
            setup: BoardEditorSessionEnumSetup::StandardSetup,
            confirmation: BoardEditorSessionEnumConfirmation::Confirmed,
        },
    )?;
    assert_eq!(session.draft().validate()?, BoardPosition::standard());
    BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Undo)?;
    assert_eq!(session.draft().fields().pieces().count(), 0);
    Ok(())
}

#[test]
fn undo_redo_restore_invalid_fields_exactly_and_keep_revisions_monotonic()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::session()?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Counters {
            halfmoves: -32,
            fullmoves: 0,
        },
    )?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::EnPassant(Some("a1".parse()?)),
    )?;
    let invalid = session.draft().fields().clone();
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Remove {
            square: "e1".parse()?,
        },
    )?;
    let missing = session.draft().fields().clone();
    let revision = session.draft().revision();
    BoardEditorFixture::select(
        &mut session,
        BoardEditorSessionEnumSelection::Palette {
            color: BoardEnumColor::White,
            kind: BoardEnumPieceKind::Queen,
        },
    )?;
    BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Undo)?;
    assert_eq!(session.draft().fields(), &invalid);
    assert_eq!(session.draft().revision(), revision.next()?);
    assert_eq!(session.selection(), BoardEditorSessionEnumSelection::None);
    BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Redo)?;
    assert_eq!(session.draft().fields(), &missing);
    assert_eq!(session.draft().revision(), revision.next()?.next()?);
    assert_eq!(
        session.draft().status(),
        BoardEditorValidationEnumStatus::Invalid
    );
    Ok(())
}

#[test]
fn a_changed_edit_clears_redo_while_noops_and_selection_preserve_it()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::session()?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Remove {
            square: "e2".parse()?,
        },
    )?;
    BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Undo)?;
    let revision = session.draft().revision();
    assert_eq!(
        BoardEditorFixture::edit(
            &mut session,
            BoardEditorSessionEnumEdit::Turn(BoardEnumColor::White)
        )?,
        BoardEditorSessionEnumOutcome::Unchanged
    );
    assert_eq!(session.draft().revision(), revision);
    BoardEditorFixture::select(
        &mut session,
        BoardEditorSessionEnumSelection::Palette {
            color: BoardEnumColor::White,
            kind: BoardEnumPieceKind::Bishop,
        },
    )?;
    assert!(session.has_redo());
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Turn(BoardEnumColor::Black),
    )?;
    assert!(!session.has_redo());
    assert!(matches!(
        BoardEditorFixture::apply(&mut session, BoardEditorSessionEnumOperation::Redo),
        Err(BoardEditorSessionError::NoRedo)
    ));
    Ok(())
}

#[test]
fn persisted_invalid_drafts_restore_fields_identity_and_revision_with_fresh_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::session()?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Counters {
            halfmoves: i64::MIN,
            fullmoves: 0,
        },
    )?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Remove {
            square: "e1".parse()?,
        },
    )?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::EnPassant(Some("d4".parse()?)),
    )?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::GrantCastling("d3".parse()?),
    )?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Turn(BoardEnumColor::Black),
    )?;
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Variant(BoardEnumVariant::Chess960),
    )?;
    BoardEditorFixture::select(
        &mut session,
        BoardEditorSessionEnumSelection::Palette {
            color: BoardEnumColor::Black,
            kind: BoardEnumPieceKind::Rook,
        },
    )?;
    let saved = serde_json::to_string(session.draft())?;
    assert!(!saved.contains("undo"));
    assert!(!saved.contains("redo"));
    assert!(!saved.contains("selection"));
    assert!(saved.contains("\"status\":\"invalid\""));
    let draft: BoardEditorDraft = serde_json::from_str(&saved)?;
    assert_eq!(&draft, session.draft());
    let mut reopened = BoardEditorSession::from_draft(draft);
    assert_eq!(reopened.selection(), BoardEditorSessionEnumSelection::None);
    assert!(!reopened.has_undo());
    assert!(!reopened.has_redo());
    assert!(matches!(
        BoardEditorFixture::apply(&mut reopened, BoardEditorSessionEnumOperation::Undo),
        Err(BoardEditorSessionError::NoUndo)
    ));
    assert!(reopened.draft().play_prefill().is_err());
    let reset = BoardEditorSession::from_draft(session.into_draft());
    assert!(!reset.has_undo());
    assert!(!reset.has_redo());
    Ok(())
}

#[test]
fn persistence_revalidates_claimed_status_and_rejects_ambiguous_placements()
-> Result<(), Box<dyn std::error::Error>> {
    let session = BoardEditorFixture::session()?;
    let saved = serde_json::to_string(session.draft())?;
    assert!(
        serde_json::from_str::<BoardEditorDraft>(
            &saved.replace("\"status\":\"valid\"", "\"status\":\"invalid\"")
        )
        .is_err()
    );
    let fields = r#"{"variant":"standard","pieces":[{"square":0,"color":"white","kind":"king"},{"square":0,"color":"black","kind":"king"}],"turn":"white","castling_rights":[],"en_passant":null,"halfmoves":0,"fullmoves":1}"#;
    let duplicate = format!(
        "{{\"id\":\"{}\",\"revision\":\"0\",\"fields\":{fields},\"status\":\"invalid\"}}",
        BoardEditorFixture::id()?
    );
    assert!(serde_json::from_str::<BoardEditorDraft>(&duplicate).is_err());
    let duplicate_right = saved.replace(
        "\"castling_rights\":[0,7,56,63]",
        "\"castling_rights\":[0,0,7,56,63]",
    );
    assert_ne!(duplicate_right, saved);
    assert!(serde_json::from_str::<BoardEditorDraft>(&duplicate_right).is_err());
    assert!(
        serde_json::from_str::<BoardEditorDraft>(&saved.replace("\"square\":0", "\"square\":64"))
            .is_err()
    );
    assert!(
        serde_json::from_str::<BoardEditorDraft>(
            &saved.replace("\"variant\":\"standard\"", "\"variant\":\"atomic\"")
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn draft_identifiers_and_revisions_validate_their_own_semantics()
-> Result<(), Box<dyn std::error::Error>> {
    for invalid in [
        "00000000-0000-0000-0000-000000000000",
        "123E4567-E89B-42D3-A456-426614174146",
        "123e4567e89b42d3a456426614174146",
    ] {
        assert!(invalid.parse::<BoardEditorDraftId>().is_err());
        assert!(
            serde_json::from_str::<BoardEditorDraftId>(&serde_json::to_string(invalid)?).is_err()
        );
    }
    for invalid in ["00", "+1", "-1", "18446744073709551616"] {
        assert!(
            serde_json::from_str::<BoardEditorDraftRevision>(&serde_json::to_string(invalid)?)
                .is_err()
        );
    }
    assert_eq!(
        serde_json::from_str::<BoardEditorDraftId>(&serde_json::to_string(
            &BoardEditorFixture::id()?
        )?)?,
        BoardEditorFixture::id()?
    );
    assert!(matches!(
        BoardEditorDraftRevision::new(u64::MAX).next(),
        Err(BoardEditorDraftError::RevisionExhausted)
    ));
    Ok(())
}

#[test]
fn stale_wrong_identity_and_exhausted_revisions_cannot_change_content_or_history()
-> Result<(), Box<dyn std::error::Error>> {
    let mut session = BoardEditorFixture::session()?;
    let stale = BoardEditorSessionRequest {
        draft: session.draft().id(),
        expected_revision: session.draft().revision(),
        operation: BoardEditorSessionEnumOperation::Undo,
    };
    BoardEditorFixture::edit(
        &mut session,
        BoardEditorSessionEnumEdit::Remove {
            square: "e2".parse()?,
        },
    )?;
    let before = session.draft().clone();
    assert!(matches!(
        session.apply(stale),
        Err(BoardEditorSessionError::Draft(
            BoardEditorDraftError::StaleRevision { .. }
        ))
    ));
    assert!(matches!(
        session.apply(BoardEditorSessionRequest {
            draft: "123e4567-e89b-42d3-a456-426614174147".parse()?,
            expected_revision: session.draft().revision(),
            operation: BoardEditorSessionEnumOperation::Undo
        }),
        Err(BoardEditorSessionError::WrongDraft { .. })
    ));
    assert_eq!(session.draft(), &before);
    assert!(session.has_undo());
    let draft = BoardEditorDraft::from_parts(
        before.id(),
        BoardEditorDraftRevision::new(u64::MAX),
        before.fields().clone(),
    );
    let mut exhausted = BoardEditorSession::from_draft(draft);
    let before = exhausted.draft().clone();
    assert!(matches!(
        BoardEditorFixture::edit(
            &mut exhausted,
            BoardEditorSessionEnumEdit::Turn(BoardEnumColor::Black)
        ),
        Err(BoardEditorSessionError::Draft(
            BoardEditorDraftError::RevisionExhausted
        ))
    ));
    assert_eq!(exhausted.draft(), &before);
    assert!(!exhausted.has_undo());
    assert!(!exhausted.has_redo());
    let draft = BoardEditorDraft::from_parts(
        before.id(),
        BoardEditorDraftRevision::new(u64::MAX - 1),
        before.fields().clone(),
    );
    let mut last_revision = BoardEditorSession::from_draft(draft);
    BoardEditorFixture::edit(
        &mut last_revision,
        BoardEditorSessionEnumEdit::Turn(BoardEnumColor::Black),
    )?;
    let fields = last_revision.draft().fields().clone();
    assert!(last_revision.has_undo());
    assert!(matches!(
        BoardEditorFixture::apply(&mut last_revision, BoardEditorSessionEnumOperation::Undo),
        Err(BoardEditorSessionError::Draft(
            BoardEditorDraftError::RevisionExhausted
        ))
    ));
    assert_eq!(last_revision.draft().fields(), &fields);
    assert!(last_revision.has_undo());
    assert!(!last_revision.has_redo());
    Ok(())
}

#[test]
fn declared_variant_fen_import_makes_a_separate_valid_draft()
-> Result<(), Box<dyn std::error::Error>> {
    let current = BoardEditorFixture::session()?;
    let before = current.draft().clone();
    let imported = BoardEditorDraft::from_fen(
        "123e4567-e89b-42d3-a456-426614174147".parse()?,
        "4k3/8/8/8/8/8/8/RR2K3 w B - 50 99",
        BoardEnumVariant::Chess960,
    )?;
    assert_ne!(imported.id(), current.draft().id());
    assert_eq!(imported.fields().variant(), BoardEnumVariant::Chess960);
    assert_eq!(
        imported.validated_fen()?,
        "4k3/8/8/8/8/8/8/RR2K3 w B - 50 99"
    );
    assert!(
        BoardEditorDraft::from_fen(
            imported.id(),
            "8/8/8/8/8/8/8/8 w - - 0 1",
            BoardEnumVariant::Standard
        )
        .is_err()
    );
    assert_eq!(current.draft(), &before);
    Ok(())
}
#[test]
fn typed_editor_identifier_gateway_preserves_uuid_and_refuses_nil()
-> Result<(), Box<dyn std::error::Error>> {
    let raw = uuid::Uuid::from_u128(0x10_001);
    let id = chess::board::editor::BoardEditorDraftId::try_from(raw)?;
    assert_eq!(id.as_uuid(), raw);
    assert_eq!(
        id.to_string()
            .parse::<chess::board::editor::BoardEditorDraftId>()?,
        id
    );
    assert!(chess::board::editor::BoardEditorDraftId::try_from(uuid::Uuid::nil()).is_err());
    Ok(())
}
