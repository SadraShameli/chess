use std::mem::size_of;
use std::num::{NonZeroU32, NonZeroU64};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::board::enums::BoardEnumVariant;
use crate::board::r#move::BoardMove;
use crate::board::position::BoardPosition;
use crate::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchIdentityError, EngineSearchRevision,
};
use crate::engine::search::position::{EngineSearchPosition, EngineSearchPositionError};
use crate::game::enums::{GameId, GameMoveId, GameRevision};
use crate::game::tree::{GameTree, GameTreeError};
use crate::package::digest::PackageDigest;

#[derive(Debug, Error)]
pub enum ReportJobSourceError {
    #[error("Report source limits must be positive")]
    EmptyLimit,
    #[error("The report source exceeds its position limit")]
    PositionLimit,
    #[error("The report source or retained work exceeds its cumulative memory charge limit")]
    MemoryLimit,
    #[error("The selected report position is outside its immutable line")]
    PositionIndex,
    #[error(transparent)]
    Tree(#[from] GameTreeError),
    #[error(transparent)]
    Search(#[from] EngineSearchPositionError),
    #[error(transparent)]
    Identity(#[from] EngineSearchIdentityError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportJobSourceLimits {
    positions: NonZeroU32,
    memory_bytes: NonZeroU64,
}

impl ReportJobSourceLimits {
    pub fn new(positions: u32, memory_bytes: u64) -> Result<Self, ReportJobSourceError> {
        Ok(Self {
            positions: NonZeroU32::new(positions).ok_or(ReportJobSourceError::EmptyLimit)?,
            memory_bytes: NonZeroU64::new(memory_bytes).ok_or(ReportJobSourceError::EmptyLimit)?,
        })
    }

    pub const fn positions(self) -> u32 {
        self.positions.get()
    }

    pub const fn memory_bytes(self) -> u64 {
        self.memory_bytes.get()
    }

    pub(crate) fn charge(
        self,
        retained: u64,
        additional: usize,
    ) -> Result<u64, ReportJobSourceError> {
        let next = retained
            .checked_add(
                u64::try_from(additional).map_err(|_error| ReportJobSourceError::MemoryLimit)?,
            )
            .ok_or(ReportJobSourceError::MemoryLimit)?;
        if next > self.memory_bytes() {
            return Err(ReportJobSourceError::MemoryLimit);
        }
        Ok(next)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportJobLineDigest(PackageDigest);

impl ReportJobLineDigest {
    pub const VERSION: u32 = 1;
    const DOMAIN: &[u8] = b"report-source-line-v1";

    pub const fn sha256(self) -> PackageDigest {
        self.0
    }
}

#[derive(Debug)]
pub struct ReportJobSourcePosition {
    node: GameMoveId,
    movement: Option<BoardMove>,
    position: BoardPosition,
}

impl PartialEq for ReportJobSourcePosition {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
            && self.movement == other.movement
            && self.position.variant() == other.position.variant()
            && self.position.fen() == other.position.fen()
    }
}

impl Eq for ReportJobSourcePosition {}

impl ReportJobSourcePosition {
    pub const fn node(&self) -> GameMoveId {
        self.node
    }
    pub const fn movement(&self) -> Option<BoardMove> {
        self.movement
    }
    pub const fn position(&self) -> &BoardPosition {
        &self.position
    }
}

#[derive(Debug)]
pub struct ReportJobSource {
    game: GameId,
    revision: GameRevision,
    context: EngineSearchContext,
    digest: ReportJobLineDigest,
    positions: Box<[ReportJobSourcePosition]>,
    limits: ReportJobSourceLimits,
    memory_charge_bytes: u64,
}

impl ReportJobSource {
    pub fn capture(
        game: GameId,
        revision: GameRevision,
        tree: &GameTree,
        tip: GameMoveId,
        limits: ReportJobSourceLimits,
    ) -> Result<Self, ReportJobSourceError> {
        let count = tree
            .line_len(tip)?
            .checked_add(1)
            .ok_or(ReportJobSourceError::PositionLimit)?;
        if u32::try_from(count).map_or(true, |count| count > limits.positions()) {
            return Err(ReportJobSourceError::PositionLimit);
        }
        let allocation = count
            .checked_mul(size_of::<ReportJobSourcePosition>() + size_of::<GameMoveId>())
            .ok_or(ReportJobSourceError::MemoryLimit)?;
        let mut memory_charge_bytes = limits.charge(0, size_of::<Self>())?;
        memory_charge_bytes = limits.charge(memory_charge_bytes, allocation)?;
        let path = tree.path_to(tip)?;
        let mut digest = Sha256::new();
        digest.update(ReportJobLineDigest::DOMAIN);
        digest.update(
            u64::try_from(count)
                .map_err(|_error| ReportJobSourceError::PositionLimit)?
                .to_be_bytes(),
        );
        let mut positions = Vec::with_capacity(count);
        for id in path {
            let node = tree.node(id)?;
            let fen = node.position().fen();
            memory_charge_bytes = limits.charge(memory_charge_bytes, fen.len())?;
            digest.update(node.id().as_uuid().as_bytes());
            digest.update([match node.position().variant() {
                BoardEnumVariant::Standard => 0,
                BoardEnumVariant::Chess960 => 1,
            }]);
            digest.update(
                u64::try_from(fen.len())
                    .map_err(|_error| ReportJobSourceError::MemoryLimit)?
                    .to_be_bytes(),
            );
            digest.update(fen.as_bytes());
            match node.movement() {
                Some(movement) => {
                    let uci = movement.uci(node.position().variant());
                    digest.update([1]);
                    digest.update(
                        u64::try_from(uci.len())
                            .map_err(|_error| ReportJobSourceError::MemoryLimit)?
                            .to_be_bytes(),
                    );
                    digest.update(uci.as_bytes());
                }
                None => digest.update([0]),
            }
            positions.push(ReportJobSourcePosition {
                node: id,
                movement: node.movement(),
                position: node.position().clone(),
            });
        }
        Ok(Self {
            game,
            revision,
            context: EngineSearchContext::new(
                EngineSearchGameId::try_from(game.as_uuid())?,
                EngineSearchRevision::new(revision.value()),
            ),
            digest: ReportJobLineDigest(PackageDigest::from_bytes(digest.finalize().into())),
            positions: positions.into_boxed_slice(),
            limits,
            memory_charge_bytes,
        })
    }

    pub const fn game(&self) -> GameId {
        self.game
    }
    pub const fn revision(&self) -> GameRevision {
        self.revision
    }
    pub const fn context(&self) -> EngineSearchContext {
        self.context
    }
    pub const fn digest(&self) -> ReportJobLineDigest {
        self.digest
    }
    pub const fn limits(&self) -> ReportJobSourceLimits {
        self.limits
    }
    pub const fn memory_charge_bytes(&self) -> u64 {
        self.memory_charge_bytes
    }
    pub fn positions(&self) -> &[ReportJobSourcePosition] {
        &self.positions
    }
    pub fn tip(&self) -> GameMoveId {
        self.positions[self.positions.len() - 1].node
    }

    pub fn is_same_line(&self, other: &Self) -> bool {
        self.game == other.game && self.digest == other.digest && self.positions == other.positions
    }

    pub(crate) fn search_position(
        &self,
        index: u32,
    ) -> Result<EngineSearchPosition, ReportJobSourceError> {
        let index = usize::try_from(index).map_err(|_error| ReportJobSourceError::PositionIndex)?;
        let prefix = self
            .positions
            .get(..=index)
            .ok_or(ReportJobSourceError::PositionIndex)?;
        EngineSearchPosition::new(
            self.positions[0].position.clone(),
            prefix.iter().filter_map(|node| {
                node.movement
                    .map(|movement| movement.uci(node.position.variant()))
            }),
        )
        .map_err(Into::into)
    }
}
