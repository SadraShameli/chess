use chess::storage::migration::StorageMigrationRunner;

#[test]
fn every_released_schema_has_an_upgrade_fixture_and_none_is_required_before_the_first_release()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/migrations/released-schemas.json"
    ))?;
    let released = fixture
        .get("released")
        .and_then(serde_json::Value::as_array)
        .ok_or("the fixture lists released schema versions")?;
    let embedded = StorageMigrationRunner::embedded_versions()?;
    assert!(!embedded.is_empty(), "the embedded migrations must exist");
    for version in released {
        let version = version.as_str().ok_or("a released version is text")?;
        assert!(
            embedded.iter().any(|candidate| candidate == version),
            "released schema {version} is no longer embedded"
        );
    }
    if released.is_empty() {
        assert_eq!(
            fixture.get("note").and_then(serde_json::Value::as_str).map(|note| note.contains("no upgrade path")),
            Some(true)
        );
    }
    Ok(())
}
