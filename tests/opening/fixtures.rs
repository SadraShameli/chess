use std::num::NonZeroUsize;

use chess::board::r#move::BoardMove;
use chess::board::position::BoardPosition;
use chess::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataKnownTags, GameMetadataText,
};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::opening::corpus::{
    OpeningCorpusGameId, OpeningCorpusLimits, OpeningCorpusRecord, OpeningCorpusRecordInput,
};
use chess::package::digest::PackageDigest;
use chess::storage::dataset_package::StorageDatasetPackageSourceIdentity;

pub(super) struct OpeningFixtureGame {
    pub(super) initial: BoardPosition,
    pub(super) mainline: Vec<BoardMove>,
    pub(super) metadata: GameMetadata,
    pub(super) result: Option<NotationGameIdentityEnumResult>,
    pub(super) time_control: Option<GameMetadataText>,
}

impl OpeningFixtureGame {
    pub(super) fn new(
        moves: &[&str],
        tags: GameMetadataKnownTags,
        result: Option<NotationGameIdentityEnumResult>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let initial = BoardPosition::standard();
        let mut position = initial.clone();
        let mut mainline = Vec::new();
        for value in moves {
            let movement = position.parse_uci(value)?;
            position = position.play(movement)?;
            mainline.push(movement);
        }
        Ok(Self {
            initial,
            mainline,
            metadata: GameMetadata::new(tags, Vec::new())?,
            result,
            time_control: Some(GameMetadataText::new("300+3".to_owned())?),
        })
    }

    pub(super) fn tags() -> Result<GameMetadataKnownTags, Box<dyn std::error::Error>> {
        Ok(GameMetadataKnownTags {
            white: Some(GameMetadataText::new("Alice".to_owned())?),
            black: Some(GameMetadataText::new("Bob".to_owned())?),
            date: Some(GameMetadataDate::new(Some(2026), Some(10), Some(17))?),
            white_elo: Some(2200),
            black_elo: Some(2000),
            ..GameMetadataKnownTags::default()
        })
    }

    pub(super) fn record(
        &self,
        id: u64,
    ) -> Result<OpeningCorpusRecord<'_>, Box<dyn std::error::Error>> {
        Ok(OpeningCorpusRecord::new(
            self.input(id)?,
            Self::limits()?,
            || false,
        )?)
    }

    pub(super) fn input(
        &self,
        id: u64,
    ) -> Result<OpeningCorpusRecordInput<'_>, Box<dyn std::error::Error>> {
        Ok(OpeningCorpusRecordInput {
            id: OpeningCorpusGameId::new(id)?,
            initial: &self.initial,
            mainline: &self.mainline,
            metadata: &self.metadata,
            result: self.result,
            time_control: self.time_control.as_ref(),
        })
    }

    pub(super) fn limits() -> Result<OpeningCorpusLimits, Box<dyn std::error::Error>> {
        Ok(OpeningCorpusLimits::new(
            NonZeroUsize::new(1000).ok_or("record limit")?,
            NonZeroUsize::new(256).ok_or("ply limit")?,
            NonZeroUsize::new(16 * 1024 * 1024).ok_or("memory limit")?,
            NonZeroUsize::new(1_000_000).ok_or("work limit")?,
        ))
    }

    pub(super) fn source(
        name: &str,
    ) -> Result<StorageDatasetPackageSourceIdentity, Box<dyn std::error::Error>> {
        Ok(StorageDatasetPackageSourceIdentity::new(
            name.to_owned(),
            "v1".to_owned(),
            PackageDigest::hash_slice(name.as_bytes()),
        )?)
    }
}
