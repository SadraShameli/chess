use std::future::Future;
use std::io::Cursor;
use std::pin::Pin;
use std::time::Duration;

use chess::board::enums::BoardEnumColor;
use chess::board::position::BoardPosition;
use chess::board::start::{BoardStart, BoardStartIndex};
use chess::capacity::budget::CapacityBudget;
use chess::capacity::executor::CapacityExecutor;
use chess::clock::duration::ClockInstant;
use chess::clock::state::ClockState;
use chess::clock::time_control::ClockTimeControl;
use chess::game::enums::{
    GameEnumEngineReadiness, GameEnumMode, GameEnumResult, GameEnumSeat, GameId, GameMoveId,
};
use chess::game::library_organization::{
    GameLibraryOrganizationEnumMutation, GameLibraryOrganizationFolderId,
    GameLibraryOrganizationFolderTarget, GameLibraryOrganizationGameTarget,
    GameLibraryOrganizationName, GameLibraryOrganizationReceipt, GameLibraryOrganizationRequest,
    GameLibraryOrganizationRevision, GameLibraryOrganizationTagId, GameLibraryOrganizationTagSet,
    GameLibraryOrganizationTimestamp,
};
use chess::game::mate_proof::{GameMateProof, GameMateProofBudget, GameMateProofControl};
use chess::game::session::{
    GameMetadata, GameMetadataDate, GameMetadataKnownTags, GameMetadataOpeningCode,
    GameMetadataText, GameSeats, GameSession, GameSessionMoveRequest, GameSessionOptions,
};
use chess::game::storage_repository::GameStorageRevision;
use chess::game::tree::GameTree;
use chess::notation::game::{NotationGame, NotationGameParts};
use chess::notation::game_identity::NotationGameIdentityEnumResult;
use chess::notation::pgn_import::NotationPgnImport;
use chess::storage::document_store::StorageDocumentStore;
use chess::storage::game_document::StorageGameDocument;
use chess::storage::library_prepared::StorageLibraryEnumContentChange;
use chess::storage::library_query::{StorageLibraryQuery, StorageLibraryQueryOptions};
use chess::storage::library_repository::{
    StorageLibraryAdoptionCursor, StorageLibraryContentReceipt, StorageLibraryEnumAdoptionOutcome,
    StorageLibraryEnumAdoptionPreparation, StorageLibraryEnumMutationReceipt,
    StorageLibraryRepository,
};

pub(super) type StorageLibraryFiltersTestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) struct StorageLibraryFiltersNotation {
    pub(super) start: BoardStart,
    pub(super) known: GameMetadataKnownTags,
    pub(super) plies: usize,
    pub(super) result: NotationGameIdentityEnumResult,
}

impl Default for StorageLibraryFiltersNotation {
    fn default() -> Self {
        Self {
            start: BoardStart::standard(),
            known: GameMetadataKnownTags::default(),
            plies: 0,
            result: NotationGameIdentityEnumResult::Unfinished,
        }
    }
}

struct StorageLibraryFiltersProofClock;

impl GameMateProofControl for StorageLibraryFiltersProofClock {
    fn now(&self) -> Duration {
        Duration::ZERO
    }

    fn is_cancelled(&self) -> bool {
        false
    }
}

pub(super) struct StorageLibraryFiltersFixture {
    directory: tempfile::TempDir,
    store: StorageDocumentStore,
    executor: CapacityExecutor,
}

impl StorageLibraryFiltersFixture {
    const MOVES: [&'static str; 6] = ["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"];
    const PLAYED_MOVES: [&'static str; 4] = ["f2f3", "e7e5", "g2g4", "d8h4"];

    pub(super) async fn new() -> StorageLibraryFiltersTestResult<Self> {
        let directory = tempfile::tempdir()?;
        let budget = CapacityBudget::discover()?;
        let executor = CapacityExecutor::new(budget.clone())?;
        let store =
            StorageDocumentStore::open(directory.path().join("filters.sqlite"), budget).await?;
        Ok(Self {
            directory,
            store,
            executor,
        })
    }

    pub(super) fn repository(&self) -> StorageLibraryRepository<'_> {
        StorageLibraryRepository::new(self.store.client(), &self.executor)
    }

    pub(super) fn id(value: u128) -> StorageLibraryFiltersTestResult<GameId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    fn node(value: u128) -> StorageLibraryFiltersTestResult<GameMoveId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    pub(super) fn text(value: &str) -> StorageLibraryFiltersTestResult<GameMetadataText> {
        Ok(GameMetadataText::new(value.to_owned())?)
    }

    pub(super) fn notation_body(
        id: GameId,
        options: StorageLibraryFiltersNotation,
    ) -> StorageLibraryFiltersTestResult<String> {
        if options.plies > Self::MOVES.len() {
            return Err("notation fixture exceeds its legal move sequence".into());
        }
        let root = Self::node(1)?;
        let mut tree = GameTree::new(root, options.start.position().clone(), None);
        let mut tip = root;
        for (index, uci) in Self::MOVES.iter().take(options.plies).enumerate() {
            let movement = tree.node(tip)?.position().parse_uci(uci)?;
            let node = Self::node(u128::try_from(index)? + 2)?;
            tree.append(tip, node, movement, None)?;
            tip = node;
        }
        let game = NotationGame::new(NotationGameParts {
            start: options.start,
            tree,
            mainline_tip: tip,
            metadata: GameMetadata::new(options.known, Vec::new())?,
            result: options.result,
            result_header: None,
            time_control: None,
            clocks: Vec::new(),
            warnings: Vec::new(),
        })?;
        Ok(StorageGameDocument::capture_notation(id, &game)?.json()?)
    }

    pub(super) fn parsed_body(id: GameId, source: &str) -> StorageLibraryFiltersTestResult<String> {
        let mut next = 100_u128;
        let mut parser = NotationPgnImport::new(Cursor::new(source.as_bytes()), move || {
            next += 1;
            uuid::Uuid::from_u128(next).to_string().parse()
        });
        let game = parser
            .next_game(&|| false)?
            .ok_or("missing parsed fixture game")?;
        if parser.next_game(&|| false)?.is_some() {
            return Err("fixture unexpectedly contains another game".into());
        }
        Ok(StorageGameDocument::capture_notation(id, &game)?.json()?)
    }

    pub(super) fn played_body(
        id: GameId,
        mode: GameEnumMode,
        known: GameMetadataKnownTags,
        plies: usize,
    ) -> StorageLibraryFiltersTestResult<String> {
        if plies > Self::PLAYED_MOVES.len() {
            return Err("played fixture exceeds its legal move sequence".into());
        }
        let black = match mode {
            GameEnumMode::LocalTwoPlayer => GameEnumSeat::Human,
            GameEnumMode::HumanVersusEngine => GameEnumSeat::Reckless,
        };
        let mut session = GameSession::new(GameSessionOptions {
            id,
            root: Self::node(1)?,
            start: BoardStart::standard(),
            seats: GameSeats::new(mode, GameEnumSeat::Human, black)?,
            clock: ClockState::new(ClockTimeControl::untimed(), BoardEnumColor::White),
            metadata: GameMetadata::new(known, Vec::new())?,
        })?;
        let proof = GameMateProof::new(
            GameMateProofBudget::default(),
            StorageLibraryFiltersProofClock,
        );
        session.start(
            session.revision(),
            ClockInstant::from_nanos(0),
            GameEnumEngineReadiness::Ready,
            &proof,
        )?;
        for (index, uci) in Self::PLAYED_MOVES.iter().take(plies).enumerate() {
            let position = session.tree().node(session.live())?.position();
            let color = position.turn();
            let movement = position.parse_uci(uci)?;
            session.accept_move(
                GameSessionMoveRequest {
                    expected_revision: session.revision(),
                    seat: session.seats().seat(color),
                    color,
                    node: Self::node(u128::try_from(index)? + 2)?,
                    movement,
                    received_at: ClockInstant::from_millis(u64::try_from(index)? + 1)?,
                },
                &proof,
            )?;
        }
        Ok(StorageGameDocument::capture(&session, ClockInstant::from_millis(10)?)?.json()?)
    }

    pub(super) async fn write(
        &self,
        id: GameId,
        expected: GameStorageRevision,
        body: String,
        change: StorageLibraryEnumContentChange,
    ) -> StorageLibraryFiltersTestResult<StorageLibraryContentReceipt> {
        let repository = self.repository();
        let prepared = repository.prepare_write(id, expected, body, change).await?;
        let mut pending = repository.begin_save(prepared)?;
        let StorageLibraryEnumMutationReceipt::Saved(receipt) = pending.wait().await? else {
            return Err("unexpected save receipt".into());
        };
        Ok(receipt)
    }

    pub(super) async fn insert(
        &self,
        id: GameId,
        body: String,
        timestamp: i64,
    ) -> StorageLibraryFiltersTestResult<StorageLibraryContentReceipt> {
        self.write(
            id,
            GameStorageRevision::INITIAL,
            body,
            StorageLibraryEnumContentChange::Changed(GameLibraryOrganizationTimestamp::new(
                timestamp,
            )?),
        )
        .await
    }

    pub(super) async fn adopt(
        &self,
        id: GameId,
        body: String,
    ) -> StorageLibraryFiltersTestResult<GameStorageRevision> {
        let client = self.store.client();
        client.save(id.as_uuid(), 0, body.clone()).await?;
        let before = client
            .read(id.as_uuid())
            .await?
            .ok_or("missing adoption source")?;
        let revision = GameStorageRevision::new(before.revision());
        assert_eq!(before.body(), body);
        drop(before);
        let repository = self.repository();
        let mut cursor = StorageLibraryAdoptionCursor::start();
        let candidate = loop {
            let page = repository.adoption_page(cursor).await?;
            if let Some(candidate) = page
                .candidates()
                .iter()
                .find(|entry| entry.id() == id.as_uuid())
            {
                break *candidate;
            }
            cursor = page.next_cursor().ok_or("missing adoption candidate")?;
        };
        assert_eq!(candidate.content_revision(), revision);
        let StorageLibraryEnumAdoptionPreparation::Prepared(prepared) =
            repository.prepare_adoption(candidate).await?
        else {
            return Err("adoption did not prepare the real game envelope".into());
        };
        let mut pending = repository.begin_adopt(prepared)?;
        assert!(matches!(
            pending.wait().await?,
            StorageLibraryEnumMutationReceipt::Adopted(
                StorageLibraryEnumAdoptionOutcome::Inserted(_)
            )
        ));
        let after = client
            .read(id.as_uuid())
            .await?
            .ok_or("missing adopted document")?;
        assert_eq!(after.body(), body);
        assert_eq!(after.revision(), revision.value());
        let recovered = repository.load(id).await?.ok_or("missing adopted record")?;
        assert_eq!(recovered.modified(), None);
        assert_eq!(recovered.content_revision(), revision);
        Ok(revision)
    }

    pub(super) async fn expect_ids(
        &self,
        options: StorageLibraryQueryOptions,
        expected: &[u128],
    ) -> StorageLibraryFiltersTestResult {
        let page = self
            .repository()
            .query(StorageLibraryQuery::new(options)?)
            .await?;
        assert!(page.next_cursor().is_none());
        let mut actual: Vec<_> = page
            .entries()
            .iter()
            .map(|entry| entry.id().as_uuid())
            .collect();
        actual.sort_unstable();
        let mut expected: Vec<_> = expected
            .iter()
            .copied()
            .map(uuid::Uuid::from_u128)
            .collect();
        expected.sort_unstable();
        assert_eq!(actual, expected);
        Ok(())
    }

    pub(super) async fn organize(
        &self,
        mutation: GameLibraryOrganizationEnumMutation,
    ) -> StorageLibraryFiltersTestResult<GameLibraryOrganizationReceipt> {
        Ok(self
            .repository()
            .organize(GameLibraryOrganizationRequest::new(
                GameLibraryOrganizationTimestamp::new(2_000)?,
                mutation,
            )?)
            .await?)
    }

    pub(super) async fn target(
        &self,
        value: u128,
    ) -> StorageLibraryFiltersTestResult<GameLibraryOrganizationGameTarget> {
        let id = Self::id(value)?;
        let loaded = self
            .repository()
            .load(id)
            .await?
            .ok_or("missing organization target")?;
        Ok(GameLibraryOrganizationGameTarget::new(
            id,
            loaded.organization().revision(),
        ))
    }

    pub(super) fn folder(
        value: u128,
    ) -> StorageLibraryFiltersTestResult<GameLibraryOrganizationFolderId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    pub(super) fn tag(
        value: u128,
    ) -> StorageLibraryFiltersTestResult<GameLibraryOrganizationTagId> {
        Ok(uuid::Uuid::from_u128(value).to_string().parse()?)
    }

    pub(super) fn tags(
        values: &[u128],
    ) -> StorageLibraryFiltersTestResult<GameLibraryOrganizationTagSet> {
        Ok(GameLibraryOrganizationTagSet::new(
            values
                .iter()
                .copied()
                .map(Self::tag)
                .collect::<StorageLibraryFiltersTestResult<_>>()?,
        )?)
    }

    pub(super) async fn organization(&self) -> StorageLibraryFiltersTestResult {
        let parent = self
            .organize(GameLibraryOrganizationEnumMutation::CreateFolder(
                Self::folder(1_001)?,
                None,
                GameLibraryOrganizationName::new("Study".to_owned())?,
            ))
            .await?;
        self.organize(GameLibraryOrganizationEnumMutation::CreateFolder(
            Self::folder(1_002)?,
            Some(GameLibraryOrganizationFolderTarget::new(
                Self::folder(1_001)?,
                parent.revision(),
            )),
            GameLibraryOrganizationName::new("Replies".to_owned())?,
        ))
        .await?;
        self.organize(GameLibraryOrganizationEnumMutation::CreateFolder(
            Self::folder(1_003)?,
            None,
            GameLibraryOrganizationName::new("Other".to_owned())?,
        ))
        .await?;
        for (game, folder) in [(1, 1_001), (2, 1_002), (5, 1_003)] {
            self.organize(GameLibraryOrganizationEnumMutation::MoveToFolder(
                self.target(game).await?,
                Some(GameLibraryOrganizationFolderTarget::new(
                    Self::folder(folder)?,
                    GameLibraryOrganizationRevision::ZERO,
                )),
            ))
            .await?;
        }
        for value in [2_001, 2_002, 2_003] {
            self.organize(GameLibraryOrganizationEnumMutation::CreateTag(
                Self::tag(value)?,
                GameLibraryOrganizationName::new(format!("Tag {value}"))?,
            ))
            .await?;
        }
        for (game, tags) in [(1, vec![2_001, 2_002]), (2, vec![2_001]), (5, vec![2_002])] {
            self.organize(GameLibraryOrganizationEnumMutation::ReplaceTags(
                self.target(game).await?,
                Self::tags(&tags)?,
            ))
            .await?;
        }
        for game in [1, 5] {
            self.organize(GameLibraryOrganizationEnumMutation::SetFavorite(
                self.target(game).await?,
                true,
            ))
            .await?;
        }
        Ok(())
    }

    pub(super) async fn cursor_records(&self) -> StorageLibraryFiltersTestResult {
        for value in 1..=101 {
            let id = Self::id(value)?;
            let body = Self::notation_body(
                id,
                StorageLibraryFiltersNotation {
                    plies: usize::try_from(value % 3)?,
                    ..StorageLibraryFiltersNotation::default()
                },
            )?;
            self.insert(id, body, 1_000).await?;
        }
        Ok(())
    }

    pub(super) async fn representative_records(&self) -> StorageLibraryFiltersTestResult {
        let id = Self::id(1)?;
        self.insert(id, Self::parsed_body(id, "[White \"  Élodie  \"]\n[Black \"Straße\"]\n[Date \"2024.02.29\"]\n[Opening \"Café Gambit\"]\n[ECO \"C20\"]\n[Result \"1-0\"]\n\n1. e4 e5 1-0")?, 1_000).await?;
        for (value, options) in [
            (
                2,
                StorageLibraryFiltersNotation {
                    known: GameMetadataKnownTags {
                        white: Some(Self::text("Bob")?),
                        black: Some(Self::text("  E\u{301}LODIE ")?),
                        date: Some(GameMetadataDate::new(Some(2024), None, Some(29))?),
                        opening: Some(Self::text("Cafe\u{301} Defense")?),
                        eco: Some(GameMetadataOpeningCode::new("B01".to_owned())?),
                        ..GameMetadataKnownTags::default()
                    },
                    plies: 4,
                    result: NotationGameIdentityEnumResult::Completed(GameEnumResult::Victory(
                        BoardEnumColor::Black,
                    )),
                    ..StorageLibraryFiltersNotation::default()
                },
            ),
            (
                3,
                StorageLibraryFiltersNotation {
                    known: GameMetadataKnownTags {
                        white: Some(Self::text(" ? ")?),
                        black: Some(Self::text(" \u{2003} ")?),
                        date: Some(GameMetadataDate::new(None, None, None)?),
                        opening: Some(Self::text("?")?),
                        ..GameMetadataKnownTags::default()
                    },
                    result: NotationGameIdentityEnumResult::Completed(GameEnumResult::Draw),
                    ..StorageLibraryFiltersNotation::default()
                },
            ),
            (
                4,
                StorageLibraryFiltersNotation {
                    plies: 1,
                    ..StorageLibraryFiltersNotation::default()
                },
            ),
        ] {
            let id = Self::id(value)?;
            self.insert(id, Self::notation_body(id, options)?, 1_000)
                .await?;
        }
        let known = GameMetadataKnownTags {
            white: Some(Self::text("Straße")?),
            black: Some(Self::text("ALICE")?),
            date: Some(GameMetadataDate::new(Some(2023), Some(5), Some(1))?),
            opening: Some(Self::text("Café Gambit")?),
            eco: Some(GameMetadataOpeningCode::new("C20".to_owned())?),
            ..GameMetadataKnownTags::default()
        };
        let id = Self::id(5)?;
        self.insert(
            id,
            Self::played_body(id, GameEnumMode::LocalTwoPlayer, known, 2)?,
            1_000,
        )
        .await?;
        let id = Self::id(6)?;
        self.insert(
            id,
            Self::played_body(
                id,
                GameEnumMode::HumanVersusEngine,
                GameMetadataKnownTags {
                    white: Some(Self::text("Engine guest")?),
                    black: Some(Self::text("Reckless")?),
                    ..GameMetadataKnownTags::default()
                },
                4,
            )?,
            1_000,
        )
        .await?;
        for (value, start) in [
            (7, BoardStart::chess960(BoardStartIndex::STANDARD)?),
            (
                8,
                BoardStart::dfrc(BoardStartIndex::STANDARD, BoardStartIndex::try_from(0)?)?,
            ),
            (9, BoardStart::custom(BoardPosition::standard())),
            (
                10,
                BoardStart::custom(
                    BoardStart::chess960(BoardStartIndex::try_from(0)?)?
                        .position()
                        .clone(),
                ),
            ),
        ] {
            let id = Self::id(value)?;
            self.insert(
                id,
                Self::notation_body(
                    id,
                    StorageLibraryFiltersNotation {
                        start,
                        ..StorageLibraryFiltersNotation::default()
                    },
                )?,
                1_000,
            )
            .await?;
        }
        Ok(())
    }

    pub(super) fn close(self) -> Pin<Box<dyn Future<Output = StorageLibraryFiltersTestResult>>> {
        Box::pin(async move {
            self.store.close().await?;
            self.executor.shutdown().await?;
            self.directory.close()?;
            Ok(())
        })
    }
}
