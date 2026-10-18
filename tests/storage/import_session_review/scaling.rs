use crate::storage::import_session::StorageImportSessionError;
use crate::storage::import_session::stage::tests::review::support::StorageImportSessionReviewFixture;

#[derive(Debug, Clone, Copy)]
enum StorageImportSessionReviewScalingEnumBucket {
    Identical,
    Distinct,
    ConflictingComplete,
}

#[derive(Debug)]
struct StorageImportSessionReviewScalingObservation {
    bucket: StorageImportSessionReviewScalingEnumBucket,
    games: u64,
    valid: u64,
    duplicates: u64,
    selected: u64,
    read_calls: u64,
    read_bytes: u64,
    write_calls: u64,
    write_bytes: u64,
    projections: u64,
    candidate_pages: u64,
    candidate_reads: u64,
}

impl StorageImportSessionReviewScalingEnumBucket {
    const ALL: [Self; 3] = [Self::Identical, Self::Distinct, Self::ConflictingComplete];
    const BATCHES: [usize; 3] = [8, 16, 32];
    const CONFLICTING_BATCHES: [usize; 2] = [32, 128];
    const MAX_FOURFOLD_READ_GROWTH: u64 = 6;
    const BLACK_AND_DATE: &'static str = "[Black \"Bob\"]\n[Date \"2026.10.17\"]\n";

    fn source(self, count: usize) -> Result<String, StorageImportSessionError> {
        let white_moves = ["e4", "d4", "Nf3", "c4"];
        let black_moves = ["e5", "d5", "Nf6", "c5", "g6", "b6", "e6", "a6"];
        let mut source = String::new();
        for index in 0..count {
            let (white, black) = match self {
                Self::Identical | Self::ConflictingComplete => {
                    (white_moves.first(), black_moves.first())
                }
                Self::Distinct => (
                    white_moves.get(index / black_moves.len()),
                    black_moves.get(index % black_moves.len()),
                ),
            };
            let white = white.ok_or(StorageImportSessionError::Arithmetic)?;
            let black = black.ok_or(StorageImportSessionError::Arithmetic)?;
            let player = match self {
                Self::Identical | Self::Distinct => "Alice".to_owned(),
                Self::ConflictingComplete => format!("Alice {index}"),
            };
            source.push_str(&format!("[White \"{player}\"]\n"));
            source.push_str(Self::BLACK_AND_DATE);
            source.push_str(&format!("1. {white} {black} *\n"));
        }
        Ok(source)
    }

    fn counts(self, games: u64) -> Option<(u64, u64)> {
        match self {
            Self::Identical => games.checked_sub(1).map(|duplicates| (duplicates, 1)),
            Self::Distinct | Self::ConflictingComplete => Some((0, games)),
        }
    }
}

#[tokio::test]
async fn review_conflicting_complete_identity_bucket_avoids_pairwise_reads()
-> Result<(), Box<dyn std::error::Error>> {
    let mut observations =
        Vec::with_capacity(StorageImportSessionReviewScalingEnumBucket::CONFLICTING_BATCHES.len());
    for count in StorageImportSessionReviewScalingEnumBucket::CONFLICTING_BATCHES {
        observations.push(
            StorageImportSessionReviewScalingObservation::record(
                StorageImportSessionReviewScalingEnumBucket::ConflictingComplete,
                count,
            )
            .await?,
        );
    }
    assert!(
        observations
            .iter()
            .all(StorageImportSessionReviewScalingObservation::is_consistent)
    );
    let first = observations
        .first()
        .ok_or(StorageImportSessionError::State)?;
    let last = observations
        .last()
        .ok_or(StorageImportSessionError::State)?;
    let budget = first
        .read_calls
        .checked_mul(StorageImportSessionReviewScalingEnumBucket::MAX_FOURFOLD_READ_GROWTH)
        .ok_or(StorageImportSessionError::Arithmetic)?;
    eprintln!("PGN conflicting complete identity operations: {observations:?}");
    assert!(
        last.read_calls <= budget,
        "fourfold complete-identity input exceeded bounded read growth: {observations:?}"
    );
    Ok(())
}

impl StorageImportSessionReviewScalingObservation {
    const READS_PER_GAME: u64 = 48;
    const FIXED_READS: u64 = 64;
    const READ_BYTES_PER_GAME: u64 = 16 * 1024;
    const FIXED_READ_BYTES: u64 = 8 * 1024;

    async fn record(
        bucket: StorageImportSessionReviewScalingEnumBucket,
        count: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let games = u64::try_from(count).map_err(|_error| StorageImportSessionError::Arithmetic)?;
        let fixture = StorageImportSessionReviewFixture::new(false).await?;
        let mut session = fixture.session(&bucket.source(count)?).await?;
        let prepared = session.prepare().await;
        let drained = session.cancel_retained().await;
        let summary = prepared.as_ref().ok().map(|summary| **summary);
        let observation = summary.map(|summary| Self {
            bucket,
            games,
            valid: summary.valid(),
            duplicates: summary.duplicates(),
            selected: summary.selected(),
            read_calls: fixture.counters.reads.value(),
            read_bytes: fixture.counters.read_bytes.value(),
            write_calls: fixture.counters.writes.value(),
            write_bytes: fixture.counters.write_bytes.value(),
            projections: fixture.counters.projections.value(),
            candidate_pages: fixture.counters.candidate_pages.value(),
            candidate_reads: fixture.counters.candidate_reads.value(),
        });
        drop(session);
        fixture.finish().await?;
        drained?;
        prepared?;
        observation.ok_or_else(|| StorageImportSessionError::State.into())
    }

    fn is_consistent(&self) -> bool {
        let Some((duplicates, selected)) = self.bucket.counts(self.games) else {
            return false;
        };
        self.valid == self.games
            && self.duplicates == duplicates
            && self.selected == selected
            && self.projections == self.games
            && self.read_calls > 0
            && self.read_bytes > 0
            && self.write_calls > 0
            && self.write_bytes > 0
            && self.candidate_pages > 0
            && self.candidate_reads == 0
    }

    fn obeys_identical_budget(&self) -> Option<bool> {
        let reads = self
            .games
            .checked_mul(Self::READS_PER_GAME)?
            .checked_add(Self::FIXED_READS)?;
        let bytes = self
            .games
            .checked_mul(Self::READ_BYTES_PER_GAME)?
            .checked_add(Self::FIXED_READ_BYTES)?;
        Some(self.read_calls <= reads && self.read_bytes <= bytes)
    }
}

#[tokio::test]
async fn review_structural_bucket_operation_counts_are_bounded_and_observable()
-> Result<(), Box<dyn std::error::Error>> {
    let mut observations = Vec::with_capacity(
        StorageImportSessionReviewScalingEnumBucket::ALL.len()
            * StorageImportSessionReviewScalingEnumBucket::BATCHES.len(),
    );
    for bucket in StorageImportSessionReviewScalingEnumBucket::ALL {
        for count in StorageImportSessionReviewScalingEnumBucket::BATCHES {
            observations
                .push(StorageImportSessionReviewScalingObservation::record(bucket, count).await?);
        }
    }
    assert!(
        observations
            .iter()
            .all(StorageImportSessionReviewScalingObservation::is_consistent),
        "{observations:?}"
    );
    eprintln!("PGN structural-bucket review operations: {observations:?}");
    Ok(())
}

#[tokio::test]
async fn review_identical_complete_records_obey_linear_filesystem_read_budget()
-> Result<(), Box<dyn std::error::Error>> {
    for count in StorageImportSessionReviewScalingEnumBucket::BATCHES {
        let observation = StorageImportSessionReviewScalingObservation::record(
            StorageImportSessionReviewScalingEnumBucket::Identical,
            count,
        )
        .await?;
        assert!(observation.is_consistent(), "{observation:?}");
        assert_eq!(
            observation.obeys_identical_budget(),
            Some(true),
            "{observation:?}"
        );
        eprintln!("PGN identical complete identity budget: {observation:?}");
    }
    Ok(())
}

#[tokio::test]
async fn review_complete_classes_preserve_per_record_counts_copies_and_annotations()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::storage::import_session::{
        StorageImportSessionDetailRequest, StorageImportSessionEnumChoice,
        StorageImportSessionEnumMatchSource, StorageImportSessionPageRequest,
    };
    let fixture = StorageImportSessionReviewFixture::new(false).await?;
    let mut source = String::new();
    for (white, note) in [
        ("Alice", "first"),
        (" ALICE ", "second"),
        ("Bob", "third"),
        ("?", "fourth"),
        ("Alice", "first"),
        ("Bob", "sixth"),
        ("?", "seventh"),
    ] {
        source.push_str(&format!(
            "[White \"{white}\"]\n[Black \"Carol\"]\n[Date \"2026.10.17\"]\n1. e4 {{{note}}} *\n"
        ));
    }
    let mut session = fixture.session(&source).await?;
    let summary = *session.prepare().await?;
    assert_eq!(
        (summary.valid(), summary.duplicates(), summary.selected()),
        (7, 5, 2)
    );
    let page = session
        .page(StorageImportSessionPageRequest::new(0, 100, 64 * 1024)?)
        .await?;
    assert_eq!(
        page.rows()
            .iter()
            .map(|row| (row.exact_matches(), row.potential_matches()))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 0), (0, 0), (0, 3), (2, 1), (1, 1), (0, 6)]
    );
    let details = session
        .detail(StorageImportSessionDetailRequest::new(5, 100, 64 * 1024)?)
        .await?;
    assert_eq!(
        details
            .relationships()
            .iter()
            .map(|relationship| relationship.source())
            .collect::<Vec<_>>(),
        [
            StorageImportSessionEnumMatchSource::Staged { ordinal: 1 },
            StorageImportSessionEnumMatchSource::Staged { ordinal: 2 },
            StorageImportSessionEnumMatchSource::Staged { ordinal: 4 }
        ]
    );
    assert_eq!(
        details
            .relationships()
            .iter()
            .map(|relationship| relationship.differences().notes())
            .collect::<Vec<_>>(),
        [false, true, true]
    );
    assert_eq!(
        page.rows()
            .iter()
            .filter_map(|row| row.game())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        7
    );
    for ordinal in [2, 5, 7] {
        session
            .choose(ordinal, StorageImportSessionEnumChoice::KeepCopy)
            .await?;
    }
    let confirmation = session.freeze().await?;
    let committed = session.commit(confirmation).await;
    let drained = session.cancel_retained().await;
    drop(page);
    drop(details);
    drop(session);
    fixture.finish().await?;
    assert!(
        matches!(&*committed?, crate::storage::import_session::StorageImportSessionEnumCommitOutcome::Committed(receipt) if receipt.count() == 5)
    );
    drained?;
    Ok(())
}
