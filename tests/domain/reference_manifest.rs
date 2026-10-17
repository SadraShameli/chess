use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

type ManifestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[derive(Deserialize)]
struct ReferenceManifest {
    schema: u32,
    statuses: BTreeMap<String, String>,
    scenarios: Vec<ReferenceScenario>,
}

#[derive(Deserialize)]
struct ReferenceScenario {
    id: String,
    story: u32,
    scenario: u32,
    fixtures: Vec<String>,
    tests: Vec<String>,
    coverage: String,
    evidence: Option<String>,
}

struct ReferenceManifestFixture;

impl ReferenceManifestFixture {
    const MANIFEST: &str = include_str!("../fixtures/reference-manifest.json");
    const SPECIFICATION: &str = include_str!("../../specs/001-reckless-desktop-chess/spec.md");

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn specified() -> BTreeSet<(u32, u32)> {
        let mut found = BTreeSet::new();
        let mut story = None;
        let mut scenarios = false;
        for line in Self::SPECIFICATION.lines() {
            if let Some(rest) = line.strip_prefix("### User Story ") {
                story = rest
                    .split(' ')
                    .next()
                    .and_then(|number| number.parse::<u32>().ok());
                scenarios = false;
            } else if line.starts_with("**Acceptance Scenarios**") {
                scenarios = true;
            } else if line.starts_with("### ") {
                scenarios = false;
            } else if scenarios
                && let Some(story) = story
                && let Some((number, _text)) = line.split_once(". ")
                && let Ok(number) = number.parse::<u32>()
            {
                found.insert((story, number));
            }
        }
        found
    }

    fn manifest() -> ManifestResult<ReferenceManifest> {
        Ok(serde_json::from_str(Self::MANIFEST)?)
    }
}

#[test]
fn manifest_lists_exactly_the_numbered_acceptance_scenarios_once() -> ManifestResult {
    let manifest = ReferenceManifestFixture::manifest()?;
    assert_eq!(manifest.schema, 1);
    let specified = ReferenceManifestFixture::specified();
    assert!(specified.len() >= 95);
    let listed: BTreeSet<(u32, u32)> = manifest
        .scenarios
        .iter()
        .map(|entry| (entry.story, entry.scenario))
        .collect();
    assert_eq!(listed, specified);
    assert_eq!(listed.len(), manifest.scenarios.len());
    for entry in &manifest.scenarios {
        assert_eq!(entry.id, format!("US{}-{}", entry.story, entry.scenario));
    }
    Ok(())
}

#[test]
fn every_linked_fixture_test_and_evidence_path_exists_and_coverage_is_declared() -> ManifestResult {
    let manifest = ReferenceManifestFixture::manifest()?;
    let root = ReferenceManifestFixture::root();
    for entry in &manifest.scenarios {
        assert!(
            manifest.statuses.contains_key(&entry.coverage),
            "{} has undeclared coverage {}",
            entry.id,
            entry.coverage
        );
        if entry.coverage != "pending" {
            assert!(!entry.tests.is_empty(), "{} links no test", entry.id);
        }
        let paths = entry
            .fixtures
            .iter()
            .chain(entry.tests.iter())
            .chain(entry.evidence.iter());
        for path in paths {
            assert!(
                Path::new(path).is_relative() && !path.contains(".."),
                "{} links a non-repository path {path}",
                entry.id
            );
            assert!(root.join(path).exists(), "{} links missing {path}", entry.id);
        }
    }
    Ok(())
}
