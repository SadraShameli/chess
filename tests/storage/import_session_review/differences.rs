use crate::storage::import_session::stage::tests::StorageImportSessionTestFixture;
use crate::storage::import_session::{StorageImportSessionDifferences, StorageImportSessionError};

struct StorageImportSessionReviewDifferenceFixture;

impl StorageImportSessionReviewDifferenceFixture {
    const NOTES: &'static str = "1. e4 (1. d4 {a}) (1. c4 {b}) *";
    const NOTES_MOVED: &'static str = "1. e4 (1. c4 {a}) (1. d4 {b}) *";
    const NOTES_REORDERED: &'static str = "1. e4 (1. c4 {b}) (1. d4 {a}) *";
    const CLOCKS: &'static str = "1. e4 (1. d4 {[%clk 0:00:01]}) (1. c4 {[%clk 0:00:02]}) *";
    const CLOCKS_MOVED: &'static str = "1. e4 (1. c4 {[%clk 0:00:01]}) (1. d4 {[%clk 0:00:02]}) *";
    const CLOCKS_REORDERED: &'static str =
        "1. e4 (1. c4 {[%clk 0:00:02]}) (1. d4 {[%clk 0:00:01]}) *";
    const WARNINGS: &'static str = "1. e4 (1. d4 {[%unknown a]}) (1. c4 {[%unknown b]}) *";
    const WARNINGS_MOVED: &'static str = "1. e4 (1. c4 {[%unknown a]}) (1. d4 {[%unknown b]}) *";
    const WARNINGS_REORDERED: &'static str =
        "1. e4 (1. c4 {[%unknown b]}) (1. d4 {[%unknown a]}) *";

    fn compare(
        left: &str,
        right: &str,
    ) -> Result<StorageImportSessionDifferences, StorageImportSessionError> {
        let left = StorageImportSessionTestFixture::notation(left, 1)?;
        let right = StorageImportSessionTestFixture::notation(right, 100)?;
        let left_identity = left.identity()?;
        let right_identity = right.identity()?;
        if left.id() == right.id()
            || left_identity.structural_key() != right_identity.structural_key()
            || !left_identity
                .structural_key()
                .mainline_uci()
                .iter()
                .map(String::as_str)
                .eq(["e2e4"])
        {
            return Err(StorageImportSessionError::Proof);
        }
        let differences = StorageImportSessionDifferences::between(&left, &right)?;
        if !differences.tree() || differences.selected_mainline() {
            return Err(StorageImportSessionError::Proof);
        }
        Ok(differences)
    }

    fn empty_variation() -> Result<StorageImportSessionDifferences, StorageImportSessionError> {
        Self::compare("1. e4 *", "1. e4 (1. d4) *")
    }
}

#[test]
fn review_notes_follow_move_anchors_when_preorder_text_is_unchanged()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        StorageImportSessionReviewDifferenceFixture::NOTES,
        StorageImportSessionReviewDifferenceFixture::NOTES_MOVED,
    )?;
    assert!(differences.notes());
    Ok(())
}

#[test]
fn review_nags_follow_move_anchors_when_preorder_values_are_unchanged()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        "1. e4 (1. d4 $1) (1. c4 $2) *",
        "1. e4 (1. c4 $1) (1. d4 $2) *",
    )?;
    assert!(differences.nags());
    Ok(())
}

#[test]
fn review_markings_follow_move_anchors_when_preorder_values_are_unchanged()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        "1. e4 (1. d4 {[%csl Ge4]}) (1. c4 {[%csl Re4]}) *",
        "1. e4 (1. c4 {[%csl Ge4]}) (1. d4 {[%csl Re4]}) *",
    )?;
    assert!(differences.markings());
    Ok(())
}

#[test]
fn review_source_clocks_follow_move_anchors_when_preorder_values_are_unchanged()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        StorageImportSessionReviewDifferenceFixture::CLOCKS,
        StorageImportSessionReviewDifferenceFixture::CLOCKS_MOVED,
    )?;
    assert!(differences.source_clocks());
    Ok(())
}

#[test]
fn review_unknown_directives_follow_move_anchors_when_preorder_values_are_unchanged()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        StorageImportSessionReviewDifferenceFixture::WARNINGS,
        StorageImportSessionReviewDifferenceFixture::WARNINGS_MOVED,
    )?;
    assert!(differences.warnings());
    Ok(())
}

#[test]
fn review_unannotated_variation_does_not_change_notes() -> Result<(), StorageImportSessionError> {
    assert!(!StorageImportSessionReviewDifferenceFixture::empty_variation()?.notes());
    Ok(())
}

#[test]
fn review_unannotated_variation_does_not_change_nags() -> Result<(), StorageImportSessionError> {
    assert!(!StorageImportSessionReviewDifferenceFixture::empty_variation()?.nags());
    Ok(())
}

#[test]
fn review_unannotated_variation_does_not_change_markings() -> Result<(), StorageImportSessionError>
{
    assert!(!StorageImportSessionReviewDifferenceFixture::empty_variation()?.markings());
    Ok(())
}

#[test]
fn review_annotation_preserving_variation_reorder_keeps_notes_equal()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        StorageImportSessionReviewDifferenceFixture::NOTES,
        StorageImportSessionReviewDifferenceFixture::NOTES_REORDERED,
    )?;
    assert!(!differences.notes());
    Ok(())
}

#[test]
fn review_annotation_preserving_variation_reorder_keeps_source_clocks_equal()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        StorageImportSessionReviewDifferenceFixture::CLOCKS,
        StorageImportSessionReviewDifferenceFixture::CLOCKS_REORDERED,
    )?;
    assert!(!differences.source_clocks());
    Ok(())
}

#[test]
fn review_annotation_preserving_variation_reorder_keeps_unknown_directives_equal()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        StorageImportSessionReviewDifferenceFixture::WARNINGS,
        StorageImportSessionReviewDifferenceFixture::WARNINGS_REORDERED,
    )?;
    assert!(!differences.warnings());
    Ok(())
}

#[test]
fn review_deep_paths_distinguish_the_same_uci_below_different_parents()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        "1. e4 (1. d4 d5 2. Nf3 {a}) (1. c4 d5 2. Nf3 {b}) *",
        "1. e4 (1. c4 d5 2. Nf3 {a}) (1. d4 d5 2. Nf3 {b}) *",
    )?;
    assert!(differences.notes());
    let preserved = StorageImportSessionReviewDifferenceFixture::compare(
        "1. e4 (1. d4 d5 2. Nf3 {a}) (1. c4 d5 2. Nf3 {b}) *",
        "1. e4 (1. c4 d5 2. Nf3 {b}) (1. d4 d5 2. Nf3 {a}) *",
    )?;
    assert!(!preserved.notes());
    Ok(())
}

#[test]
fn review_unannotated_branch_changes_only_the_ordered_tree_category()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::empty_variation()?;
    assert!(differences.tree());
    assert!(!differences.selected_mainline());
    assert!(!differences.notes());
    assert!(!differences.nags());
    assert!(!differences.markings());
    assert!(!differences.metadata());
    assert!(!differences.warnings());
    assert!(!differences.source_clocks());
    Ok(())
}

#[test]
fn review_matching_annotations_remain_anchored_to_the_initial_position()
-> Result<(), StorageImportSessionError> {
    let left = StorageImportSessionTestFixture::notation("1. Nf3 {same} *", 1)?;
    let right = StorageImportSessionTestFixture::notation(
        "[SetUp \"1\"]\n[FEN \"rnbqkbnr/ppppppp1/7p/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 2\"]\n2. Nf3 {same} *",
        100,
    )?;
    assert_ne!(left.start().position(), right.start().position());
    let differences = StorageImportSessionDifferences::between(&left, &right)?;
    assert!(differences.tree());
    assert!(differences.notes());
    Ok(())
}

#[test]
fn review_ordered_annotations_and_directives_are_compared_within_each_node()
-> Result<(), StorageImportSessionError> {
    let left = StorageImportSessionTestFixture::notation(
        "1. e4 $1 $2 {[%csl Ge4,Re5][%unknown a][%unknown b]} *",
        1,
    )?;
    let right = StorageImportSessionTestFixture::notation(
        "1. e4 $2 $1 {[%csl Re5,Ge4][%unknown b][%unknown a]} *",
        100,
    )?;
    let differences = StorageImportSessionDifferences::between(&left, &right)?;
    assert!(!differences.tree());
    assert!(differences.nags());
    assert!(differences.markings());
    assert!(differences.warnings());
    Ok(())
}

#[test]
fn review_annotation_preserving_reorder_keeps_every_anchored_category_equal()
-> Result<(), StorageImportSessionError> {
    let differences = StorageImportSessionReviewDifferenceFixture::compare(
        "1. e4 (1. d4 $1 {a [%csl Ge4][%clk 0:00:01][%unknown a]}) (1. c4 $2 {b [%csl Re4][%clk 0:00:02][%unknown b]}) *",
        "1. e4 (1. c4 $2 {b [%csl Re4][%clk 0:00:02][%unknown b]}) (1. d4 $1 {a [%csl Ge4][%clk 0:00:01][%unknown a]}) *",
    )?;
    assert!(differences.tree());
    assert!(!differences.notes());
    assert!(!differences.nags());
    assert!(!differences.markings());
    assert!(!differences.warnings());
    assert!(!differences.source_clocks());
    Ok(())
}
