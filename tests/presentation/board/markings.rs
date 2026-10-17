use bevy_egui::egui::Modifiers;

use chess::board::enums::{BoardEnumColor, BoardEnumPieceKind, BoardEnumVariant};
use chess::board::position::BoardPosition;
use chess::board::square::BoardSquare;
use chess::desktop::dto::DesktopBoardSnapshot;
use chess::presentation::board::markings::{
    PresentationBoardAids, PresentationBoardMark, PresentationBoardMarkColor,
    PresentationBoardMarkings,
};

type PresentationBoardMarkingsResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn square(name: &str) -> Result<BoardSquare, Box<dyn std::error::Error>> {
    Ok(name.parse()?)
}

fn snapshot(fen: &str) -> Result<DesktopBoardSnapshot, Box<dyn std::error::Error>> {
    Ok(DesktopBoardSnapshot::from_position(
        &BoardPosition::from_fen(fen, BoardEnumVariant::Standard)?,
    ))
}

#[test]
fn colors_are_the_four_approved_shades_and_modifiers_pick_them() {
    let green = PresentationBoardMarkColor::default();
    assert_eq!(green, PresentationBoardMarkColor::Green);
    assert_eq!(PresentationBoardMarkColor::ALL.len(), 4);
    let none = Modifiers::NONE;
    let shift = Modifiers::SHIFT;
    let alt = Modifiers::ALT;
    let both = Modifiers {
        shift: true,
        alt: true,
        ..Modifiers::NONE
    };
    assert_eq!(
        PresentationBoardMarkColor::from_modifiers(none, green),
        green
    );
    assert_eq!(
        PresentationBoardMarkColor::from_modifiers(shift, green),
        PresentationBoardMarkColor::Red
    );
    assert_eq!(
        PresentationBoardMarkColor::from_modifiers(alt, green),
        PresentationBoardMarkColor::Blue
    );
    assert_eq!(
        PresentationBoardMarkColor::from_modifiers(both, green),
        PresentationBoardMarkColor::Yellow
    );
}

#[test]
fn repeating_a_mark_removes_it_and_a_new_color_replaces_it() -> PresentationBoardMarkingsResult {
    let mut markings = PresentationBoardMarkings::default();
    let (e2, e4) = (square("e2")?, square("e4")?);
    markings.toggle_arrow(e2, e4, PresentationBoardMarkColor::Green);
    markings.toggle_circle(e4, PresentationBoardMarkColor::Green);
    assert_eq!(markings.marks().len(), 2);
    markings.toggle_arrow(e2, e4, PresentationBoardMarkColor::Red);
    assert_eq!(markings.marks().len(), 2);
    assert!(markings.marks().contains(&PresentationBoardMark::Arrow {
        origin: e2,
        destination: e4,
        color: PresentationBoardMarkColor::Red,
    }));
    markings.toggle_arrow(e2, e4, PresentationBoardMarkColor::Red);
    markings.toggle_arrow(e4, e4, PresentationBoardMarkColor::Green);
    assert!(markings.marks().is_empty());
    Ok(())
}

#[test]
fn temporary_marks_clear_when_the_position_changes_and_the_last_move_is_recorded()
-> PresentationBoardMarkingsResult {
    let start = BoardPosition::standard();
    let after = start.play(start.parse_uci("e2e4")?)?;
    let first = DesktopBoardSnapshot::from_position(&start);
    let second = DesktopBoardSnapshot::from_position(&after);
    let mut markings = PresentationBoardMarkings::default();
    markings.observe(None, &first);
    markings.toggle_circle(square("a1")?, PresentationBoardMarkColor::Blue);
    markings.observe(Some(&first), &first);
    assert_eq!(markings.marks().len(), 1);
    markings.observe(Some(&first), &second);
    assert!(markings.marks().is_empty());
    assert_eq!(markings.last_move(), Some((square("e2")?, square("e4")?)));
    Ok(())
}

#[test]
fn captures_count_losses_since_the_first_observed_position() -> PresentationBoardMarkingsResult {
    let initial = snapshot("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1")?;
    let after = snapshot("4k3/8/8/4P3/8/8/8/4K3 b - - 0 1")?;
    let mut markings = PresentationBoardMarkings::default();
    markings.observe(None, &initial);
    assert!(markings.captures().is_empty());
    markings.observe(Some(&initial), &after);
    let captures = markings.captures();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].color, BoardEnumColor::Black);
    assert_eq!(captures[0].kind, BoardEnumPieceKind::Pawn);
    assert_eq!(captures[0].count, 1);
    Ok(())
}

#[test]
fn aids_default_to_the_approved_initial_settings() {
    let aids = PresentationBoardAids::default();
    assert!(aids.legal_dots && aids.last_move && aids.check && aids.captured && aids.coordinates);
}

#[test]
fn a_promotion_does_not_count_the_pawn_as_captured() -> PresentationBoardMarkingsResult {
    let before = snapshot("4k3/P7/8/8/8/8/8/4K3 w - - 0 1")?;
    let after = snapshot("Q3k3/8/8/8/8/8/8/4K3 b - - 0 1")?;
    let mut markings = PresentationBoardMarkings::default();
    markings.observe(None, &before);
    markings.observe(Some(&before), &after);
    assert!(markings.captures().is_empty());
    Ok(())
}

#[test]
fn resetting_the_baseline_rebases_captures_on_the_next_snapshot() -> PresentationBoardMarkingsResult
{
    let initial = snapshot("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1")?;
    let after = snapshot("4k3/8/8/4P3/8/8/8/4K3 b - - 0 1")?;
    let mut markings = PresentationBoardMarkings::default();
    markings.observe(None, &initial);
    markings.observe(Some(&initial), &after);
    assert_eq!(markings.captures().len(), 1);
    markings.reset_baseline();
    markings.observe(Some(&after), &after);
    assert!(markings.captures().is_empty());
    Ok(())
}

#[test]
fn toggles_become_requests_and_external_marks_replace_local_ones() -> PresentationBoardMarkingsResult
{
    let mut markings = PresentationBoardMarkings::default();
    let e4 = square("e4")?;
    markings.toggle_circle(e4, PresentationBoardMarkColor::Red);
    markings.clear();
    let requests = markings.take_requests();
    assert_eq!(requests.len(), 2);
    assert!(markings.take_requests().is_empty());
    let circle = PresentationBoardMark::Circle {
        square: e4,
        color: PresentationBoardMarkColor::Blue,
    };
    let game = circle.to_game().ok_or("circle converts")?;
    assert_eq!(PresentationBoardMark::from_game(game), circle);
    markings.set_external(Some((Vec::new(), vec![circle])));
    assert!(markings.has_temporary());
    markings.set_external(None);
    assert!(!markings.has_temporary());
    Ok(())
}

#[test]
fn keyboard_entry_toggles_circles_and_arrows_and_flags_invalid_squares()
-> PresentationBoardMarkingsResult {
    let mut markings = PresentationBoardMarkings::default();
    *markings.entry_origin_mut() = "e2".to_owned();
    markings.toggle_entry(false);
    assert_eq!(markings.marks().len(), 1);
    markings.toggle_entry(true);
    assert!(markings.is_entry_invalid());
    *markings.entry_destination_mut() = "e4".to_owned();
    markings.toggle_entry(true);
    assert!(!markings.is_entry_invalid());
    assert_eq!(markings.marks().len(), 2);
    Ok(())
}
