use chess::board::r#move::BoardMove;
use chess::board::position::BoardPosition;
use chess::puzzle::definition::PuzzleDefinition;
use chess::puzzle::identity::{PuzzleDefinitionIdentity, PuzzleDefinitionIdentityError};
use chess::puzzle::metadata::PuzzleDefinitionMetadata;

pub(crate) struct PuzzleFixture;

impl PuzzleFixture {
    pub(crate) fn identity() -> Result<PuzzleDefinitionIdentity, PuzzleDefinitionIdentityError> {
        PuzzleDefinitionIdentity::new("fixture".into(), "v1".into(), "tactic-1".into())
    }

    pub(crate) fn line(
        initial: &BoardPosition,
        values: &[&str],
    ) -> Result<Vec<BoardMove>, Box<dyn std::error::Error>> {
        let mut position = initial.clone();
        let mut line = Vec::new();
        for value in values {
            let movement = position.parse_uci(value)?;
            position = position.play(movement)?;
            line.push(movement);
        }
        Ok(line)
    }

    pub(crate) fn definition(
        initial: BoardPosition,
        lines: &[&[&str]],
    ) -> Result<PuzzleDefinition, Box<dyn std::error::Error>> {
        let solutions = lines
            .iter()
            .map(|line| Self::line(&initial, line))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PuzzleDefinition::new(
            Self::identity()?,
            initial,
            solutions,
            PuzzleDefinitionMetadata::default(),
        )?)
    }
}
