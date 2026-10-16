use std::collections::HashSet;

use chess::desktop::file_handle::DesktopFileEnumPurpose;
use chess::desktop::identity::DesktopRevision;
use chess::desktop::messages::{DesktopEnumCommand, DesktopMessagesEntityId};
use chess::presentation::action::registry::{PresentationActionEnumId, PresentationActionRegistry};

#[test]
fn every_registered_action_builds_its_real_native_intent_without_duplicate_identity() {
    let actions = PresentationActionRegistry::actions();
    assert!(!actions.is_empty());
    let mut visited = HashSet::new();
    let owner = DesktopMessagesEntityId::new();
    for &action in actions {
        assert!(visited.insert(action));
        assert!(!action.label().is_empty());
        let command = action.command(owner);
        match action {
            PresentationActionEnumId::Refresh => {
                assert!(matches!(command.operation(), DesktopEnumCommand::Bootstrap));
                assert_eq!(command.expected_revision(), None);
            }
            PresentationActionEnumId::SelectFile => {
                assert!(matches!(
                    command.operation(),
                    DesktopEnumCommand::FileSelect {
                        owner: selected_owner,
                        purpose: DesktopFileEnumPurpose::ReadFile,
                    } if *selected_owner == owner
                ));
                assert_eq!(command.expected_revision(), Some(DesktopRevision::ZERO));
            }
            PresentationActionEnumId::ConfirmQuit => {
                assert!(matches!(command.operation(), DesktopEnumCommand::Quit));
                assert_eq!(command.expected_revision(), None);
            }
        }
    }
}
