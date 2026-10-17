use strum::IntoEnumIterator;
use thiserror::Error;

use crate::clock::duration::{ClockDuration, ClockDurationError};
use crate::engine::capabilities::EngineIdentity;
use crate::engine::enums::EngineEnumOption;
use crate::engine::options::EngineOptionsApplied;
use crate::engine::package::identity::EnginePackageIdentity;
use crate::engine::process_provider::EngineProcess;
use crate::engine::search::enums::EngineSearchEnumState;
use crate::engine::search::limit::{EngineSearchLimit, EngineSearchLimitError};
use crate::engine::session::EngineSession;
use crate::report::scoring::ReportScoringEnumPolicyVersion;

#[derive(Debug, Error)]
pub enum ReportJobConfigurationError {
    #[error("A report requires an idle engine with complete resources and structured analysis")]
    EngineNotReady,
    #[error("Report position time must be an exact whole number of milliseconds")]
    FractionalTime,
    #[error(transparent)]
    Limit(#[from] EngineSearchLimitError),
    #[error(transparent)]
    Duration(#[from] ClockDurationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportJobBudget(EngineSearchLimit);

impl ReportJobBudget {
    pub const DEFAULT_POSITION_TIME: ClockDuration = ClockDuration::from_nanos(3_000_000_000);

    pub fn default_per_position() -> Result<Self, ReportJobConfigurationError> {
        Self::move_time(Self::DEFAULT_POSITION_TIME)
    }

    pub fn move_time(time: ClockDuration) -> Result<Self, ReportJobConfigurationError> {
        if ClockDuration::from_millis(time.as_millis())? != time {
            return Err(ReportJobConfigurationError::FractionalTime);
        }
        Ok(Self(EngineSearchLimit::move_time(time)?))
    }

    pub fn depth(depth: u32) -> Result<Self, ReportJobConfigurationError> {
        Ok(Self(EngineSearchLimit::depth(depth)?))
    }

    pub fn nodes(nodes: u64) -> Result<Self, ReportJobConfigurationError> {
        Ok(Self(EngineSearchLimit::nodes(nodes)?))
    }

    pub const fn limit(self) -> EngineSearchLimit {
        self.0
    }
}

#[derive(Debug)]
pub struct ReportJobConfiguration {
    package: EnginePackageIdentity,
    identity: EngineIdentity,
    options: EngineOptionsApplied,
    budget: ReportJobBudget,
    policy: ReportScoringEnumPolicyVersion,
}

impl ReportJobConfiguration {
    pub fn capture<P: EngineProcess>(
        engine: &EngineSession<P>,
        budget: ReportJobBudget,
        policy: ReportScoringEnumPolicyVersion,
    ) -> Result<Self, ReportJobConfigurationError> {
        let options = engine
            .applied_options()
            .filter(|options| options.resources().is_some() && !options.is_minimal())
            .ok_or(ReportJobConfigurationError::EngineNotReady)?;
        if engine.state() != EngineSearchEnumState::Idle {
            return Err(ReportJobConfigurationError::EngineNotReady);
        }
        Ok(Self {
            package: engine.package().identity(),
            identity: engine.identity().clone(),
            options: options.clone(),
            budget,
            policy,
        })
    }

    pub const fn package(&self) -> &EnginePackageIdentity {
        &self.package
    }

    pub const fn identity(&self) -> &EngineIdentity {
        &self.identity
    }

    pub const fn options(&self) -> &EngineOptionsApplied {
        &self.options
    }

    pub const fn budget(&self) -> ReportJobBudget {
        self.budget
    }

    pub const fn policy(&self) -> ReportScoringEnumPolicyVersion {
        self.policy
    }

    pub fn matches_engine<P: EngineProcess>(&self, engine: &EngineSession<P>) -> bool {
        let package = engine.package();
        let identity = engine.identity();
        self.package.target() == package.target()
            && self.package.profile() == package.profile()
            && self.package.digest() == package.digest()
            && self.identity.name() == identity.name()
            && self.identity.author() == identity.author()
            && self.identity.capabilities().version() == identity.capabilities().version()
            && EngineEnumOption::iter().all(|option| {
                self.identity.capabilities().descriptor(option)
                    == identity.capabilities().descriptor(option)
            })
            && engine.applied_options() == Some(&self.options)
    }
}
