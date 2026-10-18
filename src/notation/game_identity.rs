use std::hash::{Hash, Hasher};

use icu_casemap::CaseMapper;
use icu_normalizer::{ComposingNormalizer, DecomposingNormalizer};
use thiserror::Error;

use crate::board::enums::{BoardEnumColor, BoardEnumVariant};
use crate::board::r#move::{BoardMove, BoardMoveError};
use crate::board::position::{BoardPosition, BoardPositionIdentity};
use crate::game::enums::GameEnumResult;
use crate::game::session::{GameMetadata, GameMetadataDate, GameMetadataText};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotationGameIdentityEnumResult {
    Unfinished,
    Completed(GameEnumResult),
}

impl NotationGameIdentityEnumResult {
    pub const fn pgn_token(self) -> &'static str {
        match self {
            Self::Unfinished => "*",
            Self::Completed(GameEnumResult::Victory(BoardEnumColor::White)) => "1-0",
            Self::Completed(GameEnumResult::Victory(BoardEnumColor::Black)) => "0-1",
            Self::Completed(GameEnumResult::Draw) => "1/2-1/2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotationGameIdentityEnumMatch {
    Exact,
    Potential,
    Different,
}

#[derive(Debug, Error)]
pub enum NotationGameIdentityError {
    #[error("Invalid mainline move at zero-based index {index}: {source}")]
    Mainline {
        index: usize,
        #[source]
        source: BoardMoveError,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NotationGameIdentityStructuralKey {
    variant: BoardEnumVariant,
    initial_fen: String,
    mainline_uci: Vec<String>,
}

impl NotationGameIdentityStructuralKey {
    pub const fn variant(&self) -> BoardEnumVariant {
        self.variant
    }

    pub fn initial_fen(&self) -> &str {
        &self.initial_fen
    }

    pub fn mainline_uci(&self) -> &[String] {
        &self.mainline_uci
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotationGameIdentity {
    structural: NotationGameIdentityStructuralKey,
    position: BoardPositionIdentity,
    white: Option<String>,
    black: Option<String>,
    date: Option<GameMetadataDate>,
    result: Option<NotationGameIdentityEnumResult>,
}

impl NotationGameIdentity {
    const MISSING_PLAYER: &str = "?";

    pub fn new(
        initial: &BoardPosition,
        mainline: &[BoardMove],
        metadata: &GameMetadata,
        result: Option<NotationGameIdentityEnumResult>,
    ) -> Result<Self, NotationGameIdentityError> {
        let variant = initial.variant();
        let mut position = initial.clone();
        let mut mainline_uci = Vec::with_capacity(mainline.len());
        for (index, &movement) in mainline.iter().enumerate() {
            position = position
                .play(movement)
                .map_err(|source| NotationGameIdentityError::Mainline { index, source })?;
            mainline_uci.push(movement.uci(variant));
        }
        let known = metadata.known();
        Ok(Self {
            structural: NotationGameIdentityStructuralKey {
                variant,
                initial_fen: initial.fen(),
                mainline_uci,
            },
            position: initial.identity(),
            white: Self::canonical_player(known.white.as_ref()),
            black: Self::canonical_player(known.black.as_ref()),
            date: known.date.filter(|date| {
                date.year().is_some() || date.month().is_some() || date.day().is_some()
            }),
            result,
        })
    }

    pub const fn structural_key(&self) -> &NotationGameIdentityStructuralKey {
        &self.structural
    }

    pub const fn position_identity(&self) -> &BoardPositionIdentity {
        &self.position
    }

    pub fn white(&self) -> Option<&str> {
        self.white.as_deref()
    }

    pub fn black(&self) -> Option<&str> {
        self.black.as_deref()
    }

    pub const fn date(&self) -> Option<GameMetadataDate> {
        self.date
    }

    pub const fn result(&self) -> Option<NotationGameIdentityEnumResult> {
        self.result
    }

    pub fn compare(&self, other: &Self) -> NotationGameIdentityEnumMatch {
        if self.structural != other.structural
            || Self::conflicts(self.white(), other.white())
            || Self::conflicts(self.black(), other.black())
            || Self::conflicts(self.result, other.result)
            || self.date.zip(other.date).is_some_and(|(left, right)| {
                Self::conflicts(left.year(), right.year())
                    || Self::conflicts(left.month(), right.month())
                    || Self::conflicts(left.day(), right.day())
                    || GameMetadataDate::new(
                        left.year().or(right.year()),
                        left.month().or(right.month()),
                        left.day().or(right.day()),
                    )
                    .is_err()
            })
        {
            NotationGameIdentityEnumMatch::Different
        } else if self.has_complete_metadata() && other.has_complete_metadata() {
            NotationGameIdentityEnumMatch::Exact
        } else {
            NotationGameIdentityEnumMatch::Potential
        }
    }

    pub fn canonical_player(value: Option<&GameMetadataText>) -> Option<String> {
        let trimmed = value?.as_str().trim();
        if trimmed.is_empty() || trimmed == Self::MISSING_PLAYER {
            return None;
        }
        Some(Self::canonical_fold(trimmed))
    }

    pub fn canonical_text(value: &str) -> String {
        Self::canonical_fold(value.trim())
    }

    fn canonical_fold(value: &str) -> String {
        let decomposed = DecomposingNormalizer::new_nfd().normalize(value);
        let folded = CaseMapper::new().fold_string(&decomposed);
        ComposingNormalizer::new_nfc()
            .normalize(&folded)
            .into_owned()
    }

    fn conflicts<T: PartialEq>(left: Option<T>, right: Option<T>) -> bool {
        left.zip(right).is_some_and(|(left, right)| left != right)
    }

    fn has_complete_metadata(&self) -> bool {
        self.white.is_some()
            && self.black.is_some()
            && self.result.is_some()
            && self.date.is_some_and(|date| {
                date.year().is_some() && date.month().is_some() && date.day().is_some()
            })
    }
}

impl Hash for NotationGameIdentity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.structural.hash(state);
        self.position.hash(state);
        self.white.hash(state);
        self.black.hash(state);
        self.date
            .map(|date| (date.year(), date.month(), date.day()))
            .hash(state);
        self.result
            .map(NotationGameIdentityEnumResult::pgn_token)
            .hash(state);
    }
}
