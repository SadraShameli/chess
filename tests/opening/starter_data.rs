use std::num::NonZeroUsize;
use strum::IntoEnumIterator;

use chess::board::position::BoardPosition;
use chess::board::r#move::BoardMove;
use chess::game::session::{GameMetadata, GameMetadataKnownTags, GameMetadataText};
use chess::notation::game_identity::NotationGameIdentity;
use chess::opening::book::{OpeningBook, OpeningBookGenerator, OpeningBookLimits};
use chess::package::digest::PackageDigest;
use chess::storage::dataset_package::StorageDatasetPackageSourceIdentity;
use chess::storage::dataset_preparation::{
    StorageDatasetPreparationBuild, StorageDatasetPreparationBuilder,
    StorageDatasetPreparationEnumAdmission, StorageDatasetPreparationGame,
    StorageDatasetPreparationGames,
};
use chess::storage::dataset_selection::{
    StorageDatasetSelectionEnumComponent, StorageDatasetSelectionEnumSource,
    StorageDatasetSelectionFootprint, StorageDatasetSelectionIdentity,
    StorageDatasetSelectionInstalledArtifact, StorageDatasetSelectionPrefixes,
};

type StarterDataTestResult = Result<(), Box<dyn std::error::Error>>;

struct StarterDataFixture;

impl StarterDataFixture {
    fn identity(white: &str) -> Result<StorageDatasetSelectionIdentity, Box<dyn std::error::Error>> {
        let tags = GameMetadataKnownTags {
            white: Some(GameMetadataText::new(white.to_owned())?),
            ..GameMetadataKnownTags::default()
        };
        Ok(StorageDatasetSelectionIdentity::game(
            &NotationGameIdentity::new(
                &BoardPosition::standard(),
                &[],
                &GameMetadata::new(tags, vec![])?,
                None,
            )?,
        )?)
    }

    fn game(
        white: &str,
        record: &str,
        source: StorageDatasetSelectionEnumSource,
        title: Option<&str>,
    ) -> Result<StorageDatasetPreparationGame, Box<dyn std::error::Error>> {
        Ok(StorageDatasetPreparationGame {
            record: record.to_owned(),
            identity: Self::identity(white)?,
            source,
            white_title: title.map(str::to_owned),
            black_title: title.map(str::to_owned),
            white_elo: Some("2500".to_owned()),
            black_elo: Some("2400".to_owned()),
            time_control: Some("600+5".to_owned()),
        })
    }

    fn line(moves: &[&str]) -> Result<Vec<BoardMove>, Box<dyn std::error::Error>> {
        let mut position = BoardPosition::standard();
        let mut line = Vec::new();
        for value in moves {
            let movement = position.parse_uci(value)?;
            position = position.play(movement)?;
            line.push(movement);
        }
        Ok(line)
    }
}

#[test]
fn duplicates_keep_the_smallest_record_and_retain_every_copy_for_provenance() -> StarterDataTestResult {
    let mut games = StorageDatasetPreparationGames::new();
    let standard = StorageDatasetSelectionEnumSource::Standard;
    assert_eq!(
        games.admit("b", StarterDataFixture::game("Same", "[Event \"b\"]", standard, Some("GM"))?)?,
        StorageDatasetPreparationEnumAdmission::New
    );
    assert_eq!(
        games.admit("a", StarterDataFixture::game("Same", "[Event \"a\"]", standard, Some("GM"))?)?,
        StorageDatasetPreparationEnumAdmission::Replaced
    );
    assert_eq!(
        games.admit("c", StarterDataFixture::game("Same", "[Event \"c\"]", standard, Some("GM"))?)?,
        StorageDatasetPreparationEnumAdmission::Duplicate
    );
    assert_eq!(games.len(), 1);
    let sequences = games.finish()?;
    assert_eq!(sequences.records.len(), 1);
    assert_eq!(sequences.records[0].handle, "a");
    let mut copies = sequences.records[0].copies.clone();
    copies.sort_unstable();
    assert_eq!(copies, ["b", "c"]);
    Ok(())
}

#[test]
fn master_sequence_requires_both_titles_and_excludes_broadcasts_while_broad_keeps_every_game()
-> StarterDataTestResult {
    let mut games = StorageDatasetPreparationGames::new();
    games.admit(
        1,
        StarterDataFixture::game("Titled", "1", StorageDatasetSelectionEnumSource::Standard, Some("GM"))?,
    )?;
    games.admit(
        2,
        StarterDataFixture::game("Untitled", "2", StorageDatasetSelectionEnumSource::Standard, None)?,
    )?;
    games.admit(
        3,
        StarterDataFixture::game("Broadcast", "3", StorageDatasetSelectionEnumSource::Broadcast, Some("GM"))?,
    )?;
    let sequences = games.finish()?;
    let master: Vec<_> = sequences
        .master
        .prefix(10)?
        .candidates()
        .iter()
        .map(|candidate| *candidate.handle())
        .collect();
    assert_eq!(master, [1]);
    assert_eq!(sequences.broad.prefix(10)?.candidates().len(), 3);
    Ok(())
}

#[test]
fn generated_book_counts_first_occurrences_and_validates_as_polyglot() -> StarterDataTestResult {
    let lines = [
        StarterDataFixture::line(&["e2e4", "e7e5"])?,
        StarterDataFixture::line(&["e2e4", "c7c5"])?,
        StarterDataFixture::line(&["d2d4"])?,
        StarterDataFixture::line(&["g1f3", "g8f6", "f3g1", "f6g8", "g1f3"])?,
    ];
    let generated = OpeningBookGenerator::generate(lines.iter().map(Vec::as_slice), || false)?;
    let start = BoardPosition::standard().standard_polyglot_key()?.value();
    let at_start: Vec<_> = generated
        .moves()
        .iter()
        .filter(|record| record.key == start)
        .map(|record| record.count)
        .collect();
    assert_eq!(at_start.iter().sum::<u64>(), 4);
    assert_eq!(at_start.iter().copied().max(), Some(2));
    let checksum = PackageDigest::hash_slice(generated.bytes());
    let source = StorageDatasetPackageSourceIdentity::new("masters".into(), "1".into(), checksum)?;
    let limits = OpeningBookLimits::new(NonZeroUsize::new(1 << 20).ok_or("empty limit")?)?;
    let book = OpeningBook::validate(generated.bytes(), source, checksum, limits, || false)?;
    assert_eq!(book.record_count(), generated.moves().len());
    let study = book.study(&BoardPosition::standard(), || false)?;
    assert_eq!(study.moves().len(), 3);
    assert_eq!(study.rejected_illegal_records(), 0);
    Ok(())
}

#[test]
fn overflow_weights_scale_with_nearest_rounding_and_a_minimum_of_one() -> StarterDataTestResult {
    assert_eq!(OpeningBookGenerator::weight(65_535, 65_535)?, 65_535);
    assert_eq!(OpeningBookGenerator::weight(3, 100)?, 3);
    assert_eq!(OpeningBookGenerator::weight(100_000, 200_000)?, 32_768);
    assert_eq!(OpeningBookGenerator::weight(200_000, 200_000)?, 65_535);
    assert_eq!(OpeningBookGenerator::weight(1, 200_000)?, 1);
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("fixture builder failure")]
struct StarterDataBuilderError;

struct StarterDataBuilder {
    builds: usize,
}

impl StorageDatasetPreparationBuilder for StarterDataBuilder {
    type Error = StarterDataBuilderError;

    fn build(
        &mut self,
        prefixes: StorageDatasetSelectionPrefixes,
    ) -> Result<StorageDatasetSelectionFootprint, Self::Error> {
        self.builds += 1;
        let artifacts = StorageDatasetSelectionEnumComponent::iter()
            .map(|component| {
                let bytes = if component == StorageDatasetSelectionEnumComponent::Master
                    && prefixes.master() > 90_000
                {
                    900_000_000
                } else {
                    1_000
                };
                StorageDatasetSelectionInstalledArtifact::new(
                    format!("{component:?}"),
                    Some(component),
                    bytes,
                    10,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_error| StarterDataBuilderError)?;
        StorageDatasetSelectionFootprint::from_artifacts(artifacts)
            .map_err(|_error| StarterDataBuilderError)
    }
}

#[test]
fn byte_cap_loop_reduces_the_offending_prefix_then_accepts_with_a_recorded_reduction()
-> StarterDataTestResult {
    let mut builder = StarterDataBuilder { builds: 0 };
    let initial = StorageDatasetSelectionPrefixes::new(100_000, 250_000, 200_000)?;
    let outcome = StorageDatasetPreparationBuild::run(&mut builder, initial)?;
    assert_eq!(builder.builds, 2);
    assert_eq!(outcome.reductions.len(), 1);
    assert_eq!(outcome.prefixes.master(), 90_000);
    assert_eq!(outcome.prefixes.broad(), 250_000);
    Ok(())
}
