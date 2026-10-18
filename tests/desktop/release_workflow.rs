use std::fs;
use std::path::Path;

use chess::update::release::{UpdateReleaseCheck, UpdateReleaseCheckError, UpdateReleaseState};

fn template(name: &str) -> String {
    fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".github/workflow-templates")
            .join(name),
    )
    .unwrap_or_default()
}

#[test]
fn tags_must_be_exact_stable_versions_that_match_every_version_source() {
    assert!(UpdateReleaseCheck::tag_version("v1.2.3").is_ok());
    for bad in ["1.2.3", "v1.2", "v1.2.3-rc.1", "v01.2.3", "v1.2.3+build"] {
        assert_eq!(
            UpdateReleaseCheck::tag_version(bad),
            Err(UpdateReleaseCheckError::Tag(bad.to_owned()))
        );
    }
    assert!(UpdateReleaseCheck::versions_match("v1.2.3", "1.2.3", "1.2.3", "1.2.3").is_ok());
    assert!(matches!(
        UpdateReleaseCheck::versions_match("v1.2.3", "1.2.3", "1.2.4", "1.2.3"),
        Err(UpdateReleaseCheckError::Mismatch {
            what: "package",
            ..
        })
    ));
}

#[test]
fn an_incomplete_unverified_or_unfunded_release_is_never_published() {
    let complete = UpdateReleaseState {
        draft: true,
        assets_expected: 4,
        assets_uploaded: 4,
        all_verified: true,
        allowance_remaining: true,
    };
    assert!(UpdateReleaseCheck::publication_allowed(complete).is_ok());
    for refused in [
        UpdateReleaseState {
            draft: false,
            ..complete
        },
        UpdateReleaseState {
            assets_uploaded: 3,
            ..complete
        },
        UpdateReleaseState {
            assets_expected: 0,
            assets_uploaded: 0,
            ..complete
        },
        UpdateReleaseState {
            all_verified: false,
            ..complete
        },
        UpdateReleaseState {
            allowance_remaining: false,
            ..complete
        },
    ] {
        assert!(UpdateReleaseCheck::publication_allowed(refused).is_err());
    }
}

#[test]
fn the_templates_are_inert_minimal_and_publish_only_after_a_draft() {
    let verify = template("verify.yml");
    let release = template("release.yml");
    assert!(verify.contains("cancel-in-progress: true"));
    assert!(verify.contains("contents: read"));
    assert!(!verify.contains("secrets."));
    assert!(verify.contains("retention-days: 1"));
    assert!(release.contains("cancel-in-progress: false"));
    assert!(release.contains("retention-days: 1"));
    assert!(!release.contains("pull_request"));
    let draft = release.find("--draft --verify-tag");
    let publish = release.find("--draft=false");
    assert!(matches!((draft, publish), (Some(first), Some(second)) if first < second));
    assert!(!release.contains("actions/cache"));
    assert!(
        !Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".github/workflows")
            .exists()
    );
}
