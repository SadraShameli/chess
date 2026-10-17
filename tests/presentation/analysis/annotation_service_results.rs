use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::task::Poll;

use crate::clock::duration::ClockInstant;
use crate::desktop::annotation_commands::{
    DesktopAnnotationCommandsEnumAction, DesktopAnnotationCommandsEnumResult,
};
use crate::desktop::application::DesktopApplicationReceipt;
use crate::desktop::library_service::tests::fixture::{
    DesktopLibraryServiceFixture, DesktopLibraryServiceTestResult,
};
use crate::desktop::library_service::{
    DesktopLibraryServiceEnumAction, DesktopLibraryServiceEnumDisposition,
    DesktopLibraryServiceEnumResult, DesktopLibraryServiceEnumSaveReceipt,
    DesktopLibraryServiceError, DesktopLibraryServiceOperation, DesktopLibraryServiceReceipt,
    DesktopLibraryServiceRequest,
};
use crate::game::enums::GameEnumMarkingColor;
use crate::game::move_node::GameMoveNodeMarking;
use crate::presentation::analysis::annotations::{
    PresentationAnalysisAnnotations, PresentationAnalysisAnnotationsEnumDestination,
    PresentationAnalysisAnnotationsEnumEffect, PresentationAnalysisAnnotationsEnumGuardChoice,
    PresentationAnalysisAnnotationsEnumMarkingInput,
};
use crate::presentation::analysis::editor_intent::{
    PresentationAnalysisEditorError, PresentationAnalysisEditorIntent,
};
use crate::presentation::analysis::variation_tree::PresentationAnalysisVariationTree;

impl DesktopLibraryServiceFixture {
    fn panel_knight_move(
        snapshot: &crate::desktop::game_editor::DesktopGameEditorPublication,
    ) -> DesktopLibraryServiceTestResult<crate::board::r#move::BoardMove> {
        let origin = "g1".parse()?;
        let destination = "f3".parse()?;
        Ok(snapshot
            .board()
            .legal_moves()
            .iter()
            .find(|candidate| {
                candidate.movement().origin() == Some(origin)
                    && candidate.movement().destination() == destination
            })
            .ok_or("missing legal knight branch")?
            .movement())
    }

    fn submit_panel_intent(
        &self,
        intent: &PresentationAnalysisEditorIntent,
        application: &DesktopApplicationReceipt,
    ) -> DesktopLibraryServiceTestResult<DesktopLibraryServiceReceipt> {
        let command = intent.clone().admit(application.clone())?;
        let (application, expected, action) = command.into_parts();
        Ok(self.port.try_submit(DesktopLibraryServiceRequest::new(
            application,
            DesktopLibraryServiceOperation::new(
                self.workspace,
                DesktopLibraryServiceEnumAction::Editor { expected, action },
            ),
        ))?)
    }

    async fn panel_response(
        &mut self,
        reply: &mut DesktopLibraryServiceReceipt,
    ) -> DesktopLibraryServiceTestResult<
        Result<DesktopLibraryServiceEnumResult, Arc<DesktopLibraryServiceError>>,
    > {
        tokio::select! {
            result = reply.wait() => Ok(result),
            result = Box::pin(self.service.run()) => Err(format!("service ended: {result:?}").into()),
            result = Box::pin(self.registry.run()) => Err(format!("registry ended: {result:?}").into()),
        }
    }
}

#[tokio::test]
async fn actual_saved_results_finish_both_content_kinds_without_another_checkpoint()
-> DesktopLibraryServiceTestResult {
    for notation in [true, false] {
        let mut fixture = DesktopLibraryServiceFixture::new().await?;
        let observation: DesktopLibraryServiceTestResult<_> = async {
            let revision = if notation { fixture.seed(4400, true).await? } else {
                let mut session = DesktopLibraryServiceFixture::played(4400)?;
                session.begin_analysis(session.revision(), ClockInstant::from_nanos(0))?;
                fixture.seed_played(&session).await?
            };
            let publication = fixture.open(4400, revision).await?;
            let expected = publication.context();
            let mut panel = PresentationAnalysisAnnotations::new();
            panel.synchronize(&publication, None);
            panel.set_notes(expected, "saved through the actual service".to_owned())?;
            let switch = fixture.receipt()?;
            panel.request_switch(switch.request_id(), PresentationAnalysisAnnotationsEnumDestination::Navigate(publication.root()))?;
            let application = fixture.receipt()?;
            let intent = panel.save_notes(application.request_id())?.ok_or("missing save intent")?;
            panel.attach_receipt(&intent, application.clone())?;
            let mut reply = fixture.submit_panel_intent(&intent, &application)?;
            let result = fixture.drive(&mut reply).await?;
            let DesktopLibraryServiceEnumResult::Saved { snapshot, receipt } = &result else { return Err("missing actual saved result".into()); };
            let correct_kind = matches!((notation, receipt), (true, DesktopLibraryServiceEnumSaveReceipt::Notation { .. }) | (false, DesktopLibraryServiceEnumSaveReceipt::Played(_)));
            let foreign = fixture.receipt()?;
            let foreign_refused = matches!(panel.observe_service_result(&foreign, expected, &result), Err(PresentationAnalysisEditorError::ForeignReceipt));
            let stale_refused = matches!(panel.observe_service_result(&application, snapshot.context(), &result), Err(PresentationAnalysisEditorError::UnexpectedReply));
            panel.synchronize(snapshot, None);
            let completion = panel.observe_service_result(&application, expected, &result)?;
            let correct_switch = matches!(completion, Some(PresentationAnalysisAnnotationsEnumEffect::Command(ref command)) if command.request_id() == switch.request_id() && command.expected() == snapshot.context());
            Ok((correct_kind, foreign_refused, stale_refused, correct_switch, panel.pending_intent().is_none(), !panel.has_guard(), !panel.has_notes_changes(), fixture.wall.calls.load(Ordering::SeqCst)))
        }.await;
        fixture.close().await?;
        let (kind, foreign, stale, switch, completed, unguarded, clean, writes) = observation?;
        assert!(kind && foreign && stale && switch && completed && unguarded && clean);
        assert_eq!(writes, 1);
    }
    Ok(())
}

#[tokio::test]
async fn actual_unchanged_markings_finish_without_a_write_or_checkpoint()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let revision = fixture.seed(4401, true).await?;
        let publication = fixture.open(4401, revision).await?;
        let marking = GameMoveNodeMarking::circle(GameEnumMarkingColor::Green, "e4".parse()?);
        let DesktopLibraryServiceEnumResult::Saved { snapshot, .. } = fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: publication.context(),
                action: DesktopAnnotationCommandsEnumAction::SaveMarkings {
                    markings: vec![marking],
                },
            })
            .await?
        else {
            return Err("missing marking save".into());
        };
        let mut panel = PresentationAnalysisAnnotations::new();
        panel.synchronize(&snapshot, None);
        panel.marking_input(
            snapshot.context(),
            PresentationAnalysisAnnotationsEnumMarkingInput::Toggle(marking),
        )?;
        let application = fixture.receipt()?;
        let intent = panel
            .save_markings(application.request_id())?
            .ok_or("missing unchanged marking intent")?;
        panel.attach_receipt(&intent, application.clone())?;
        let mut reply = fixture.submit_panel_intent(&intent, &application)?;
        let result = fixture.drive(&mut reply).await?;
        let unchanged = matches!(
            &result,
            DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Edited {
                change: crate::game::annotations::GameAnnotationsEnumChange::Unchanged,
                ..
            })
        );
        let effect = panel.observe_service_result(&application, snapshot.context(), &result)?;
        Ok((
            unchanged,
            effect.is_none(),
            panel.pending_intent().is_none(),
            panel
                .markings()
                .is_some_and(|view| view.temporary().is_empty()),
            fixture.wall.calls.load(Ordering::SeqCst),
        ))
    }
    .await;
    fixture.close().await?;
    let (unchanged, no_effect, completed, cleared, writes) = observation?;
    assert!(unchanged && no_effect && completed && cleared);
    assert_eq!(writes, 1);
    Ok(())
}

#[tokio::test]
async fn actual_pending_snapshot_keeps_the_original_guard_until_its_save_finishes()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let revision = fixture.seed(4402, true).await?;
        let publication = fixture.open(4402, revision).await?;
        let expected = publication.context();
        let mut panel = PresentationAnalysisAnnotations::new();
        panel.synchronize(&publication, None);
        panel.set_notes(expected, "waiting on real capacity".to_owned())?;
        panel.request_switch(
            fixture.receipt()?.request_id(),
            PresentationAnalysisAnnotationsEnumDestination::Navigate(publication.root()),
        )?;
        let application = fixture.receipt()?;
        let intent = panel
            .save_notes(application.request_id())?
            .ok_or("missing save")?;
        panel.attach_receipt(&intent, application.clone())?;
        let lease = fixture.budget.try_lease(1)?;
        let mut reply = fixture.submit_panel_intent(&intent, &application)?;
        let mut running = Box::pin(fixture.drive(&mut reply));
        let waiting =
            poll_fn(|context| Poll::Ready(running.as_mut().poll(context).is_pending())).await;
        drop(running);
        let pending = fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected,
                action: DesktopAnnotationCommandsEnumAction::Snapshot,
            })
            .await?;
        let actual_pending = matches!(
            pending,
            DesktopLibraryServiceEnumResult::PendingSnapshot { .. }
        );
        let no_effect = panel
            .observe_service_result(&application, expected, &pending)?
            .is_none();
        let guarded = panel.has_guard() && panel.pending_intent().is_some();
        let blocked = matches!(
            panel.resolve_guard(
                PresentationAnalysisAnnotationsEnumGuardChoice::Discard,
                fixture.receipt()?.request_id()
            ),
            Err(PresentationAnalysisEditorError::Busy)
        );
        panel.set_notes(expected, "newer unsaved draft".to_owned())?;
        drop(lease);
        let saved = fixture.drive(&mut reply).await?;
        let no_release = panel
            .observe_service_result(&application, expected, &saved)?
            .is_none();
        Ok((
            waiting,
            actual_pending,
            no_effect,
            guarded,
            blocked,
            no_release,
            panel.notes() == Some("newer unsaved draft"),
            panel.has_guard(),
            panel.pending_intent().is_none(),
        ))
    }
    .await;
    fixture.close().await?;
    let (
        waiting,
        pending,
        no_effect,
        guarded,
        blocked,
        no_release,
        newer,
        retained_guard,
        completed,
    ) = observation?;
    assert!(
        waiting
            && pending
            && no_effect
            && guarded
            && blocked
            && no_release
            && newer
            && retained_guard
            && completed
    );
    Ok(())
}

#[tokio::test]
async fn service_retained_failure_uses_a_fresh_receipt_and_rejects_a_foreign_error()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let revision = fixture.seed(4403, true).await?;
        let publication = fixture.open(4403, revision).await?;
        let expected = publication.context();
        let mut panel = PresentationAnalysisAnnotations::new();
        panel.synchronize(&publication, None);
        panel.set_notes(expected, "one retained edit".to_owned())?;
        let application = fixture.receipt()?;
        let intent = panel.save_notes(application.request_id())?.ok_or("missing save")?;
        panel.attach_receipt(&intent, application.clone())?;
        let memory = fixture.fill_memory()?;
        let mut reply = fixture.submit_panel_intent(&intent, &application)?;
        let error = fixture.panel_response(&mut reply).await?.err().ok_or("publication unexpectedly succeeded")?;
        let retained = matches!(error.disposition(), DesktopLibraryServiceEnumDisposition::Retained { request, expected: Some(context) } if request == application.request_id() && context == expected);
        let foreign = fixture.receipt()?;
        let foreign_refused = matches!(panel.observe_service_error(&foreign, expected, &error), Err(PresentationAnalysisEditorError::ForeignReceipt));
        panel.observe_service_error(&application, expected, &error)?;
        let cannot_discard = matches!(panel.discard_draft(), Err(PresentationAnalysisEditorError::Busy));
        let retry_application = fixture.receipt()?;
        let retry = panel.retry_save(retry_application.request_id())?.ok_or("missing retained retry")?;
        let fresh = retry.request_id() == retry_application.request_id()
            && retry.request_id() != application.request_id()
            && retry.expected() == expected
            && matches!(retry.action(), DesktopAnnotationCommandsEnumAction::RetrySave);
        panel.attach_receipt(&retry, retry_application.clone())?;
        drop(memory);
        let mut retried = fixture.submit_panel_intent(&retry, &retry_application)?;
        let result = fixture.drive(&mut retried).await?;
        let original_reconciled = matches!((reply.try_result(), &result),
            (Some(Ok(DesktopLibraryServiceEnumResult::Saved { receipt: DesktopLibraryServiceEnumSaveReceipt::Notation { receipt: original, covered: original_covered }, .. })),
             DesktopLibraryServiceEnumResult::Saved { receipt: DesktopLibraryServiceEnumSaveReceipt::Notation { receipt: actual, covered }, .. })
                if Arc::ptr_eq(&original, actual) && original_covered == *covered);
        panel.observe_service_result(&retry_application, expected, &result)?;
        Ok((retained, foreign_refused, cannot_discard, fresh, original_reconciled, panel.pending_intent().is_none(), fixture.wall.calls.load(Ordering::SeqCst)))
    }.await;
    fixture.close().await?;
    let (retained, foreign, blocked, fresh, original_reconciled, completed, writes) = observation?;
    assert!(retained && foreign && blocked && fresh && original_reconciled && completed);
    assert_eq!(writes, 1);
    Ok(())
}

#[tokio::test]
async fn retained_variation_recovery_binds_a_fresh_application_without_repeating_the_branch()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let revision = fixture.seed(4405, true).await?;
        let initial = fixture.open(4405, revision).await?;
        let DesktopLibraryServiceEnumResult::Editor(DesktopAnnotationCommandsEnumResult::Snapshot(
            snapshot,
        )) = fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: initial.context(),
                action: DesktopAnnotationCommandsEnumAction::Navigate {
                    selected: initial.root(),
                },
            })
            .await?
        else {
            return Err("missing actual root publication".into());
        };
        let expected = snapshot.context();
        let movement = DesktopLibraryServiceFixture::panel_knight_move(&snapshot)?;
        let mut panel = PresentationAnalysisVariationTree::new();
        panel.synchronize(&snapshot, None);
        let application = fixture.receipt()?;
        let intent = panel
            .append_study_move(
                application.request_id(),
                expected,
                uuid::Uuid::from_u128(4502).to_string().parse()?,
                movement,
            )?
            .ok_or("missing branch intent")?;
        panel.attach_receipt(&intent, application.clone())?;
        let pressure = fixture.fill_memory()?;
        let mut original = fixture.submit_panel_intent(&intent, &application)?;
        let failure = fixture
            .panel_response(&mut original)
            .await?
            .err()
            .ok_or("branch publication unexpectedly succeeded")?;
        panel.observe_service_error(&application, expected, &failure)?;
        let retry_application = fixture.receipt()?;
        let retry = panel
            .retry_pending(retry_application.request_id())?
            .ok_or("missing variation retry")?;
        let fresh = retry.request_id() == retry_application.request_id()
            && retry.request_id() != application.request_id()
            && retry.expected() == expected
            && matches!(
                retry.action(),
                DesktopAnnotationCommandsEnumAction::RetrySave
            );
        let attached = panel
            .attach_receipt(&retry, retry_application.clone())
            .is_ok();
        drop(pressure);
        let mut recovery = fixture.port.try_submit(DesktopLibraryServiceRequest::new(
            retry_application.clone(),
            DesktopLibraryServiceOperation::new(
                fixture.workspace,
                DesktopLibraryServiceEnumAction::Editor {
                    expected,
                    action: DesktopAnnotationCommandsEnumAction::RetrySave,
                },
            ),
        ))?;
        let result = fixture.drive(&mut recovery).await?;
        let completed = attached
            && panel
                .observe_service_result(&retry_application, expected, &result)
                .is_ok()
            && panel.pending_intent().is_none();
        let saved = matches!(
            original.try_result(),
            Some(Ok(DesktopLibraryServiceEnumResult::Saved { .. }))
        ) && matches!(result, DesktopLibraryServiceEnumResult::Saved { .. });
        Ok((
            fresh,
            attached,
            completed,
            saved,
            fixture.wall.calls.load(Ordering::SeqCst),
        ))
    }
    .await;
    fixture.close().await?;
    let (fresh, attached, completed, saved, writes) = observation?;
    assert!(saved);
    assert_eq!(writes, 1);
    assert!(
        fresh,
        "variation recovery returned the original content action and request instead of the fresh RetrySave"
    );
    assert!(attached && completed);
    Ok(())
}

#[tokio::test]
async fn tree_consumes_actual_saved_and_unchanged_service_results()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let revision = fixture.seed(4404, true).await?;
        let publication = fixture.open(4404, revision).await?;
        let mut panel = PresentationAnalysisVariationTree::new();
        panel.synchronize(&publication, None);
        let mut correct = true;
        for node in [4500_u128, 4501] {
            let current = panel.context().ok_or("missing tree context")?;
            let DesktopLibraryServiceEnumResult::Editor(
                DesktopAnnotationCommandsEnumResult::Snapshot(snapshot),
            ) = fixture
                .execute(DesktopLibraryServiceEnumAction::Editor {
                    expected: current,
                    action: DesktopAnnotationCommandsEnumAction::Navigate {
                        selected: publication.root(),
                    },
                })
                .await?
            else {
                return Err("missing authoritative board".into());
            };
            panel.synchronize(&snapshot, None);
            let expected = snapshot.context();
            let movement = DesktopLibraryServiceFixture::panel_knight_move(&snapshot)?;
            let application = fixture.receipt()?;
            let intent = panel
                .append_study_move(
                    application.request_id(),
                    expected,
                    uuid::Uuid::from_u128(node).to_string().parse()?,
                    movement,
                )?
                .ok_or("missing branch intent")?;
            panel.attach_receipt(&intent, application.clone())?;
            let mut reply = fixture.submit_panel_intent(&intent, &application)?;
            let result = fixture.drive(&mut reply).await?;
            correct &= if node == 4500 {
                matches!(&result, DesktopLibraryServiceEnumResult::Saved { .. })
            } else {
                matches!(
                    &result,
                    DesktopLibraryServiceEnumResult::Editor(
                        DesktopAnnotationCommandsEnumResult::Edited {
                            change: crate::game::annotations::GameAnnotationsEnumChange::Unchanged,
                            ..
                        }
                    )
                )
            };
            panel.observe_service_result(&application, expected, &result)?;
            correct &= panel.pending_intent().is_none();
            if node == 4501 {
                correct &= panel.context().is_some_and(|context| {
                    context.storage_revision() == expected.storage_revision()
                });
            }
        }
        Ok(correct)
    }
    .await;
    fixture.close().await?;
    let correct = observation?;
    assert!(correct);
    Ok(())
}

#[tokio::test]
async fn an_older_real_save_acknowledges_only_its_covered_edit_when_publication_is_newer()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let revision = fixture.seed(4405, true).await?;
        let publication = fixture.open(4405, revision).await?;
        let expected = publication.context();
        let mut panel = PresentationAnalysisAnnotations::new();
        panel.synchronize(&publication, None);
        panel.set_notes(expected, "the original submitted edit".to_owned())?;
        panel.request_switch(
            fixture.receipt()?.request_id(),
            PresentationAnalysisAnnotationsEnumDestination::Navigate(publication.root()),
        )?;
        let application = fixture.receipt()?;
        let intent = panel
            .save_notes(application.request_id())?
            .ok_or("missing save")?;
        panel.attach_receipt(&intent, application.clone())?;
        let mut reply = fixture.submit_panel_intent(&intent, &application)?;
        let mut saved = fixture.drive(&mut reply).await?;
        let DesktopLibraryServiceEnumResult::Saved {
            snapshot: first, ..
        } = &saved
        else {
            return Err("missing first durable result".into());
        };
        let DesktopLibraryServiceEnumResult::Saved {
            snapshot: newer, ..
        } = fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: first.context(),
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "newer canonical content".to_owned(),
                    nags: Vec::new(),
                },
            })
            .await?
        else {
            return Err("missing newer durable publication".into());
        };
        panel.set_notes(expected, "newer local draft".to_owned())?;
        panel.synchronize(&newer, None);
        let DesktopLibraryServiceEnumResult::Saved { snapshot, .. } = &mut saved else {
            return Err("lost first durable acknowledgment".into());
        };
        *snapshot = newer;
        let result = panel.observe_service_result(&application, expected, &saved);
        Ok((
            result.is_ok(),
            result.ok().is_some_and(|effect| effect.is_none()),
            panel.pending_intent().is_none(),
            panel.has_guard(),
            panel.notes() == Some("newer local draft"),
            panel.is_conflicted(),
        ))
    }
    .await;
    fixture.close().await?;
    let (acknowledged, no_release, completed, guarded, newer, conflicted) = observation?;
    assert!(acknowledged && no_release && completed && guarded && newer && conflicted);
    Ok(())
}

#[tokio::test]
async fn foreign_and_stale_real_storage_receipts_cannot_finish_a_later_save()
-> DesktopLibraryServiceTestResult {
    let mut fixture = DesktopLibraryServiceFixture::new().await?;
    let observation: DesktopLibraryServiceTestResult<_> = async {
        let revision = fixture.seed(4406, true).await?;
        let publication = fixture.open(4406, revision).await?;
        let first = fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: publication.context(),
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "first revision".to_owned(),
                    nags: Vec::new(),
                },
            })
            .await?;
        let DesktopLibraryServiceEnumResult::Saved { snapshot, .. } = &first else {
            return Err("missing first save".into());
        };
        let expected = snapshot.context();
        let mut panel = PresentationAnalysisAnnotations::new();
        panel.synchronize(snapshot, None);
        panel.set_notes(expected, "second revision".to_owned())?;
        let application = fixture.receipt()?;
        let intent = panel
            .save_notes(application.request_id())?
            .ok_or("missing second save")?;
        panel.attach_receipt(&intent, application.clone())?;
        let stale_refused = matches!(
            panel.observe_service_result(&application, expected, &first),
            Err(PresentationAnalysisEditorError::NotSaved)
        );
        let foreign_revision = fixture.seed(4407, true).await?;
        let foreign = fixture.open(4407, foreign_revision).await?;
        let foreign_saved = fixture
            .execute(DesktopLibraryServiceEnumAction::Editor {
                expected: foreign.context(),
                action: DesktopAnnotationCommandsEnumAction::SaveNotesAndNags {
                    notes: "other game".to_owned(),
                    nags: Vec::new(),
                },
            })
            .await?;
        let foreign_refused = matches!(
            panel.observe_service_result(&application, expected, &foreign_saved),
            Err(PresentationAnalysisEditorError::UnexpectedReply
                | PresentationAnalysisEditorError::ForeignReceipt)
        );
        let pending_untouched = panel
            .pending_intent()
            .is_some_and(|pending| pending.request_id() == application.request_id())
            && panel.notes() == Some("second revision");
        let mut reply = fixture.submit_panel_intent(&intent, &application)?;
        let actual = fixture.drive(&mut reply).await?;
        panel.observe_service_result(&application, expected, &actual)?;
        Ok((
            stale_refused,
            foreign_refused,
            pending_untouched,
            panel.pending_intent().is_none(),
        ))
    }
    .await;
    fixture.close().await?;
    let (stale, foreign, retained, completed) = observation?;
    assert!(stale && foreign && retained && completed);
    Ok(())
}
