use strum::IntoEnumIterator;

use chess::presentation::action::registry::{
    PresentationActionEnumId, PresentationActionEnumWorkspace,
};
use chess::presentation::analysis::configuration::PresentationAnalysisConfigurationEnumMode;
use chess::presentation::board::markings::PresentationBoardMarkColor;
use chess::presentation::game::configuration::{
    PresentationGameConfigurationEnumBonus, PresentationGameConfigurationEnumClockFormat,
};
use chess::presentation::game::overview::PresentationGameOverviewEnumStatus;
use chess::presentation::settings::update::PresentationSettingsUpdateEnumAction;

fn labels<T: IntoEnumIterator>(label: impl Fn(T) -> &'static str) -> Vec<&'static str> {
    T::iter().map(label).collect()
}

#[test]
fn presentation_labels_keep_their_spellings_and_order() {
    assert_eq!(
        labels(PresentationBoardMarkColor::label),
        ["Green", "Red", "Blue", "Yellow"]
    );
    assert_eq!(
        labels(PresentationAnalysisConfigurationEnumMode::label),
        ["Infinite", "Depth", "Nodes", "Move time", "Clock"]
    );
    assert_eq!(
        labels(PresentationGameConfigurationEnumClockFormat::label),
        [
            "Sudden death",
            "Fischer increment",
            "Staged",
            "Cyclic",
            "Simple delay",
            "Bronstein delay"
        ]
    );
    assert_eq!(
        labels(PresentationGameConfigurationEnumBonus::label),
        [
            "No increment or delay",
            "Fischer increment",
            "Simple delay",
            "Bronstein delay"
        ]
    );
}

#[test]
fn action_and_status_labels_are_unchanged() {
    assert_eq!(
        PresentationActionEnumId::Subscribe.label(),
        "Subscribe to state"
    );
    assert_eq!(
        PresentationActionEnumId::ConfirmQuit.label(),
        "Confirm quit"
    );
    assert_eq!(
        PresentationActionEnumWorkspace::OpenPalette.label(),
        "Open command palette"
    );
    assert_eq!(PresentationActionEnumWorkspace::Quit.label(), "Quit");
    assert_eq!(
        PresentationGameOverviewEnumStatus::WaitingForCapacity.label(),
        "Waiting for engine capacity"
    );
    assert_eq!(
        PresentationSettingsUpdateEnumAction::UseSessionCredential.label(),
        "Use credential for this session only"
    );
    assert_eq!(
        PresentationSettingsUpdateEnumAction::Approve.label(),
        "Approve this network for the download"
    );
}
