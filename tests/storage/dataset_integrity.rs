use std::collections::HashMap;
use std::io;

use chess::package::digest::PackageDigest;
use chess::storage::dataset_integrity::{
    StorageDatasetIntegrity, StorageDatasetIntegrityEnumFeature as Feature,
    StorageDatasetIntegrityEnumProblem as Problem, StorageDatasetIntegrityReader,
    StorageDatasetIntegrityRequirement,
};

struct StorageDatasetIntegrityFixture {
    files: HashMap<String, (u64, PackageDigest)>,
    unreadable: Vec<String>,
}

impl StorageDatasetIntegrityReader for StorageDatasetIntegrityFixture {
    fn measure(&self, artifact: &str) -> io::Result<Option<(u64, PackageDigest)>> {
        if self.unreadable.iter().any(|name| name == artifact) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        Ok(self.files.get(artifact).copied())
    }
}

fn requirement(
    feature: Feature,
    artifact: &str,
    content: &[u8],
) -> StorageDatasetIntegrityRequirement {
    StorageDatasetIntegrityRequirement {
        feature,
        artifact: artifact.to_owned(),
        bytes: u64::try_from(content.len()).unwrap_or(u64::MAX),
        checksum: PackageDigest::hash_slice(content),
    }
}

fn file(content: &[u8]) -> (u64, PackageDigest) {
    (
        u64::try_from(content.len()).unwrap_or(u64::MAX),
        PackageDigest::hash_slice(content),
    )
}

#[test]
fn intact_installs_report_no_findings() {
    let requirements = [
        requirement(Feature::Explorer, "master.db", b"games"),
        requirement(Feature::Book, "book.bin", b"book"),
        requirement(Feature::Puzzles, "puzzles.db", b"puzzles"),
    ];
    let reader = StorageDatasetIntegrityFixture {
        files: HashMap::from([
            ("master.db".to_owned(), file(b"games")),
            ("book.bin".to_owned(), file(b"book")),
            ("puzzles.db".to_owned(), file(b"puzzles")),
        ]),
        unreadable: Vec::new(),
    };
    let report = StorageDatasetIntegrity::verify(&requirements, &reader);
    assert!(report.is_clean());
    assert!(report.ensure_available(Feature::Explorer).is_ok());
}

#[test]
fn each_problem_is_typed_and_refuses_only_its_own_feature() {
    let requirements = [
        requirement(Feature::Explorer, "master.db", b"games"),
        requirement(Feature::Book, "book.bin", b"book"),
        requirement(Feature::Puzzles, "puzzles.db", b"puzzles"),
        requirement(Feature::Puzzles, "puzzle-index.db", b"index"),
    ];
    let reader = StorageDatasetIntegrityFixture {
        files: HashMap::from([
            ("master.db".to_owned(), file(b"tampered")),
            ("book.bin".to_owned(), file(b"book")),
            ("puzzle-index.db".to_owned(), file(b"idx")),
        ]),
        unreadable: Vec::new(),
    };
    let report = StorageDatasetIntegrity::verify(&requirements, &reader);
    assert_eq!(report.findings().len(), 3);
    assert!(matches!(
        report.ensure_available(Feature::Explorer),
        Err(refusal) if matches!(refusal.findings[0].problem, Problem::SizeMismatch { .. })
    ));
    assert!(report.ensure_available(Feature::Book).is_ok());
    let puzzles = report.ensure_available(Feature::Puzzles).err();
    assert!(puzzles.is_some_and(|refusal| {
        refusal.findings.len() == 2
            && refusal
                .findings
                .iter()
                .any(|finding| finding.problem == Problem::Missing)
    }));
    assert_eq!(
        report.affected().into_iter().collect::<Vec<_>>(),
        vec![Feature::Explorer, Feature::Puzzles]
    );
}

#[test]
fn checksum_changes_with_the_same_length_and_unreadable_files_are_findings() {
    let requirements = [
        requirement(Feature::Book, "book.bin", b"book"),
        requirement(Feature::Explorer, "master.db", b"games"),
    ];
    let reader = StorageDatasetIntegrityFixture {
        files: HashMap::from([("book.bin".to_owned(), file(b"bool"))]),
        unreadable: vec!["master.db".to_owned()],
    };
    let report = StorageDatasetIntegrity::verify(&requirements, &reader);
    assert!(matches!(
        report.findings()[0].problem,
        Problem::ChecksumMismatch { .. }
    ));
    assert_eq!(report.findings()[1].problem, Problem::Unreadable);
}
