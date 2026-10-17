use crate::notation::pgn_import::NotationPgnImportEnumLimit;

#[derive(Clone, Copy)]
pub(super) struct NotationPgnImportBudgetPolicy {
    pub(super) input_bytes: u64,
    pub(super) games: u64,
    pub(super) total_plies: u64,
    pub(super) game_plies: u64,
    pub(super) variation_depth: u64,
    pub(super) text_bytes: u64,
    pub(super) decoded_bytes: u64,
}

impl NotationPgnImportBudgetPolicy {
    pub(super) const fn production() -> Self {
        Self {
            input_bytes: 20 * 1024 * 1024 * 1024,
            games: 1_000_000,
            total_plies: 100_000_000,
            game_plies: 100_000,
            variation_depth: 64,
            text_bytes: 1024 * 1024,
            decoded_bytes: 64 * 1024 * 1024,
        }
    }
}

pub(super) struct NotationPgnImportBudget {
    policy: NotationPgnImportBudgetPolicy,
    input_bytes: u64,
    games: u64,
    total_plies: u64,
    game_plies: u64,
    retained: u64,
    pending: u64,
}

impl NotationPgnImportBudget {
    pub(super) fn new(policy: NotationPgnImportBudgetPolicy) -> Self {
        Self {
            policy,
            input_bytes: 0,
            games: 0,
            total_plies: 0,
            game_plies: 0,
            retained: 0,
            pending: 0,
        }
    }

    fn added(
        current: u64,
        amount: u64,
        maximum: u64,
        limit: NotationPgnImportEnumLimit,
    ) -> Result<u64, NotationPgnImportEnumLimit> {
        current
            .checked_add(amount)
            .filter(|next| *next <= maximum)
            .ok_or(limit)
    }

    pub(super) fn input_byte(&mut self) -> Result<(), NotationPgnImportEnumLimit> {
        self.input_bytes = Self::added(
            self.input_bytes,
            1,
            self.policy.input_bytes,
            NotationPgnImportEnumLimit::InputBytes,
        )?;
        Ok(())
    }

    pub(super) fn begin_game(&mut self) -> Result<(), NotationPgnImportEnumLimit> {
        self.games = Self::added(
            self.games,
            1,
            self.policy.games,
            NotationPgnImportEnumLimit::Games,
        )?;
        self.game_plies = 0;
        self.retained = 0;
        Ok(())
    }

    pub(super) fn game_ordinal(&self) -> u64 {
        self.games
    }

    pub(super) fn ply(&mut self) -> Result<(), NotationPgnImportEnumLimit> {
        let game = Self::added(
            self.game_plies,
            1,
            self.policy.game_plies,
            NotationPgnImportEnumLimit::GamePlies,
        )?;
        let total = Self::added(
            self.total_plies,
            1,
            self.policy.total_plies,
            NotationPgnImportEnumLimit::TotalPlies,
        )?;
        self.game_plies = game;
        self.total_plies = total;
        Ok(())
    }

    pub(super) fn depth(&self, depth: u64) -> Result<(), NotationPgnImportEnumLimit> {
        Self::added(
            0,
            depth,
            self.policy.variation_depth,
            NotationPgnImportEnumLimit::VariationDepth,
        )
        .map(|_| ())
    }

    pub(super) fn text(&self, bytes: u64) -> Result<(), NotationPgnImportEnumLimit> {
        Self::added(
            0,
            bytes,
            self.policy.text_bytes,
            NotationPgnImportEnumLimit::TextBytes,
        )
        .map(|_| ())
    }

    fn capacity(
        &self,
        current: usize,
        required: usize,
    ) -> Result<usize, NotationPgnImportEnumLimit> {
        self.text(
            u64::try_from(required).map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?,
        )?;
        let maximum = usize::try_from(self.policy.text_bytes)
            .map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?;
        let doubled = current
            .checked_mul(2)
            .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
        Ok(required.max(doubled).min(maximum))
    }

    pub(super) fn reserve_pending_text(
        &mut self,
        buffer: &mut Vec<u8>,
    ) -> Result<(), NotationPgnImportEnumLimit> {
        let required = buffer
            .len()
            .checked_add(1)
            .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
        let capacity = self.capacity(buffer.capacity(), required)?;
        if required > buffer.capacity() {
            self.pending(
                u64::try_from(capacity - buffer.capacity())
                    .map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?,
            )?;
            buffer.reserve_exact(capacity - buffer.len());
        }
        Ok(())
    }

    pub(super) fn reserve_retained_text(
        &mut self,
        buffer: &mut String,
        additional: usize,
    ) -> Result<(), NotationPgnImportEnumLimit> {
        let required = buffer
            .len()
            .checked_add(additional)
            .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
        let capacity = self.capacity(buffer.capacity(), required)?;
        if required > buffer.capacity() {
            self.retain(
                u64::try_from(capacity - buffer.capacity())
                    .map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?,
            )?;
            buffer.reserve_exact(capacity - buffer.len());
        }
        Ok(())
    }

    pub(super) fn reserve_retained_entry<T>(
        &mut self,
        buffer: &mut Vec<T>,
    ) -> Result<(), NotationPgnImportEnumLimit> {
        let required = buffer
            .len()
            .checked_add(1)
            .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
        if required > buffer.capacity() {
            let capacity = buffer
                .capacity()
                .checked_mul(2)
                .ok_or(NotationPgnImportEnumLimit::Arithmetic)?
                .max(required);
            let bytes = (capacity - buffer.capacity())
                .checked_mul(std::mem::size_of::<T>())
                .ok_or(NotationPgnImportEnumLimit::Arithmetic)?;
            self.retain(
                u64::try_from(bytes).map_err(|_error| NotationPgnImportEnumLimit::Arithmetic)?,
            )?;
            buffer.reserve_exact(capacity - buffer.len());
        }
        Ok(())
    }

    pub(super) fn retain(&mut self, amount: u64) -> Result<(), NotationPgnImportEnumLimit> {
        let next = Self::added(
            self.retained,
            amount,
            self.policy.decoded_bytes,
            NotationPgnImportEnumLimit::DecodedBytes,
        )?;
        Self::added(
            next,
            self.pending,
            self.policy.decoded_bytes,
            NotationPgnImportEnumLimit::DecodedBytes,
        )?;
        self.retained = next;
        Ok(())
    }

    pub(super) fn pending(&mut self, amount: u64) -> Result<(), NotationPgnImportEnumLimit> {
        let next = Self::added(
            self.pending,
            amount,
            self.policy.decoded_bytes,
            NotationPgnImportEnumLimit::DecodedBytes,
        )?;
        Self::added(
            next,
            self.retained,
            self.policy.decoded_bytes,
            NotationPgnImportEnumLimit::DecodedBytes,
        )?;
        self.pending = next;
        Ok(())
    }

    pub(super) fn clear_pending(&mut self) {
        self.pending = 0;
    }
}

#[cfg(test)]
mod tests {
    use crate::notation::pgn_import::NotationPgnImportEnumLimit;
    use crate::notation::pgn_import::budget::{
        NotationPgnImportBudget, NotationPgnImportBudgetPolicy,
    };

    #[test]
    fn exact_boundaries_succeed_and_next_growth_fails() {
        let mut budget = NotationPgnImportBudget::new(NotationPgnImportBudgetPolicy {
            input_bytes: 2,
            games: 2,
            total_plies: 2,
            game_plies: 2,
            variation_depth: 2,
            text_bytes: 2,
            decoded_bytes: 4,
        });
        assert!(budget.input_byte().is_ok());
        assert!(budget.input_byte().is_ok());
        assert!(matches!(
            budget.input_byte(),
            Err(NotationPgnImportEnumLimit::InputBytes)
        ));
        assert!(budget.begin_game().is_ok());
        assert!(budget.ply().is_ok());
        assert!(budget.ply().is_ok());
        assert!(matches!(
            budget.ply(),
            Err(NotationPgnImportEnumLimit::GamePlies)
        ));
        assert!(budget.depth(2).is_ok());
        assert!(matches!(
            budget.depth(3),
            Err(NotationPgnImportEnumLimit::VariationDepth)
        ));
        assert!(budget.text(2).is_ok());
        assert!(matches!(
            budget.text(3),
            Err(NotationPgnImportEnumLimit::TextBytes)
        ));
        assert!(budget.retain(2).is_ok());
        assert!(budget.pending(2).is_ok());
        assert!(matches!(
            budget.pending(1),
            Err(NotationPgnImportEnumLimit::DecodedBytes)
        ));
        assert!(budget.begin_game().is_ok());
        assert!(matches!(
            budget.ply(),
            Err(NotationPgnImportEnumLimit::TotalPlies)
        ));
        assert!(matches!(
            budget.begin_game(),
            Err(NotationPgnImportEnumLimit::Games)
        ));
    }

    #[test]
    fn arithmetic_overflow_fails_before_mutation() {
        let mut budget = NotationPgnImportBudget::new(NotationPgnImportBudgetPolicy::production());
        budget.retained = u64::MAX;
        assert!(budget.retain(1).is_err());
        assert_eq!(budget.retained, u64::MAX);
        budget.input_bytes = u64::MAX;
        assert!(budget.input_byte().is_err());
        assert_eq!(budget.input_bytes, u64::MAX);
    }

    #[test]
    fn releasing_pending_data_does_not_reset_retained_or_totals() {
        let mut budget = NotationPgnImportBudget::new(NotationPgnImportBudgetPolicy::production());
        assert!(budget.begin_game().is_ok());
        assert!(budget.ply().is_ok());
        assert!(budget.retain(10).is_ok());
        assert!(budget.pending(20).is_ok());
        budget.clear_pending();
        assert_eq!(budget.retained, 10);
        assert_eq!(budget.total_plies, 1);
        assert_eq!(budget.pending, 0);
    }

    #[test]
    fn decoded_capacity_is_rejected_before_buffer_allocation() {
        let mut budget = NotationPgnImportBudget::new(NotationPgnImportBudgetPolicy {
            text_bytes: 8,
            decoded_bytes: 4,
            ..NotationPgnImportBudgetPolicy::production()
        });
        let mut bytes = Vec::new();
        for byte in b"four" {
            assert!(budget.reserve_pending_text(&mut bytes).is_ok());
            bytes.push(*byte);
        }
        assert_eq!(bytes.capacity(), 4);
        assert_eq!(budget.pending, 4);
        assert!(matches!(
            budget.reserve_pending_text(&mut bytes),
            Err(NotationPgnImportEnumLimit::DecodedBytes)
        ));
        assert_eq!(bytes, b"four");
        assert_eq!(bytes.capacity(), 4);
        assert_eq!(budget.pending, 4);
    }
}
