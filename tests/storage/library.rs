use chess::game::library_organization::{
    GameLibraryOrganizationName, GameLibraryOrganizationRevision, GameLibraryOrganizationTimestamp,
};
use chess::game::session::GameMetadataDate;
use chess::storage::library_query::StorageLibraryDateInterval;

type StorageLibraryTestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn library_boundary_values_reject_noncanonical_revisions_and_invalid_names()
-> StorageLibraryTestResult {
    assert!("01".parse::<GameLibraryOrganizationRevision>().is_err());
    assert!("-1".parse::<GameLibraryOrganizationRevision>().is_err());
    assert_eq!("0".parse::<GameLibraryOrganizationRevision>()?.value(), 0);
    assert!(GameLibraryOrganizationTimestamp::new(-1).is_err());
    assert!(GameLibraryOrganizationName::new("  ".to_owned()).is_err());
    assert!(GameLibraryOrganizationName::new("bad\0name".to_owned()).is_err());
    assert_eq!(
        GameLibraryOrganizationName::new(" Study ".to_owned())?.as_str(),
        "Study"
    );
    Ok(())
}

#[test]
fn interval_requires_complete_ordered_calendar_boundaries() -> StorageLibraryTestResult {
    let full = GameMetadataDate::new(Some(2024), Some(2), Some(29))?;
    assert!(StorageLibraryDateInterval::new(full, full).is_ok());
    assert!(
        StorageLibraryDateInterval::new(GameMetadataDate::new(Some(2024), None, Some(29))?, full)
            .is_err()
    );
    assert!(
        StorageLibraryDateInterval::new(GameMetadataDate::new(Some(2025), Some(1), Some(1))?, full)
            .is_err()
    );
    Ok(())
}
