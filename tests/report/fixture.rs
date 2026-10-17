use std::num::NonZeroUsize;
use std::sync::Mutex;

use chess::board::position::BoardPosition;
use chess::capacity::budget::CapacityBudget;
use chess::clock::duration::ClockDuration;
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::host::EnginePackageHost;
use chess::engine::package::manifest::{
    EnginePackageEntry, EnginePackageEnumTarget, EnginePackageManifest,
};
use chess::engine::package::selector::{EnginePackageSelected, EnginePackageSelector};
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::resource_config::EngineResourceConfig;
use chess::engine::search::identity::{EngineSearchContext, EngineSearchTag};
use chess::engine::search::limit::EngineSearchLimit;
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::session::{
    EngineSearchSession, EngineSearchSessionEnumEvent, EngineSearchSessionRequest,
    EngineSearchSessionTerminal,
};
use chess::engine::session::EngineSession;
use chess::game::enums::{GameId, GameMoveId, GameRevision};
use chess::game::tree::GameTree;
use chess::package::digest::PackageDigest;
use chess::report::configuration::{ReportJobBudget, ReportJobConfiguration};
use chess::report::job::{ReportJob, ReportJobAttempt, ReportJobId};
use chess::report::scoring::ReportScoringEnumPolicyVersion;
use chess::report::source::{ReportJobSource, ReportJobSourceLimits};
use tokio::sync::mpsc;
use tokio::time::Instant;
use uuid::Uuid;

pub(super) type ReportFixtureResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct ReportFixtureProvider {
    receiver: Mutex<Option<mpsc::Receiver<EngineProcessLine>>>,
    sender: mpsc::Sender<EngineProcessLine>,
}

impl EngineProcessProvider for ReportFixtureProvider {
    type Process = ReportFixtureProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        let receiver = self
            .receiver
            .lock()
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .take()
            .ok_or(EngineProcessError::MissingPipe)?;
        Ok(ReportFixtureProcess {
            receiver,
            sender: self.sender.clone(),
            closed: false,
        })
    }
}

pub(super) struct ReportFixtureProcess {
    receiver: mpsc::Receiver<EngineProcessLine>,
    sender: mpsc::Sender<EngineProcessLine>,
    closed: bool,
}

impl ReportFixtureProcess {
    fn emit(&self, text: String) -> Result<(), EngineProcessError> {
        self.sender
            .try_send(EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text)?)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        Ok(())
    }
}

impl EngineProcess for ReportFixtureProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        if command == "uci" {
            self.emit(format!(
                "id name Reckless {}",
                EnginePackageManifest::VERSION
            ))?;
            self.emit(format!("id author {}", EngineIdentity::AUTHOR))?;
            for option in EngineEnumOption::ALL {
                self.emit(format!(
                    "option name {} type {}",
                    option.name(),
                    option.descriptor()
                ))?;
            }
            self.emit("uciok".to_owned())?;
        } else if command == "isready" {
            self.emit("readyok".to_owned())?;
        } else if let Some(setting) = command.strip_prefix("setoption name ") {
            let (name, value) = setting
                .split_once(" value ")
                .ok_or(EngineProcessError::InvalidCommand)?;
            let unit = if name == EngineEnumOption::Hash.name() {
                " MB"
            } else if name == EngineEnumOption::MoveOverhead.name() {
                " ms"
            } else {
                ""
            };
            self.emit(format!("info string set {name} to {value}{unit}"))?;
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        self.receiver
            .recv()
            .await
            .ok_or(EngineProcessError::Closed(EngineProcessEnumPipe::Stdout))
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.closed = true;
        Ok(())
    }

    async fn shutdown_retained(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }

    fn is_shutdown_complete(&self) -> bool {
        self.closed
    }

    async fn terminate(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.shutdown().await
    }
}

pub(super) struct ReportFixture {
    directory: tempfile::TempDir,
    search: EngineSearchSession<ReportFixtureProcess>,
    sender: mpsc::Sender<EngineProcessLine>,
    capacity: CapacityBudget,
}

impl ReportFixture {
    pub(super) fn game(value: u128) -> ReportFixtureResult<GameId> {
        Ok(Uuid::from_u128(value).to_string().parse()?)
    }

    pub(super) fn node(value: u128) -> ReportFixtureResult<GameMoveId> {
        Ok(Uuid::from_u128(value).to_string().parse()?)
    }

    pub(super) fn tree(moves: &[&str]) -> ReportFixtureResult<GameTree> {
        Self::tree_from(BoardPosition::standard(), moves)
    }

    pub(super) fn tree_from(
        initial: BoardPosition,
        moves: &[&str],
    ) -> ReportFixtureResult<GameTree> {
        let mut tree = GameTree::new(Self::node(1)?, initial, None);
        let mut parent = tree.root();
        for (offset, text) in moves.iter().enumerate() {
            let id = Self::node(
                u128::try_from(offset)?
                    .checked_add(2)
                    .ok_or("Node overflow")?,
            )?;
            let movement = tree.node(parent)?.position().parse_uci(text)?;
            parent = tree.append(parent, id, movement, None)?;
        }
        Ok(tree)
    }

    pub(super) fn source(tree: &GameTree) -> ReportFixtureResult<ReportJobSource> {
        let tip = tree.nodes().last().ok_or("Missing root")?.id();
        Ok(ReportJobSource::capture(
            Self::game(1)?,
            GameRevision::ZERO,
            tree,
            tip,
            ReportJobSourceLimits::new(128, 8 * 1024 * 1024)?,
        )?)
    }

    pub(super) async fn new() -> ReportFixtureResult<Self> {
        let directory = tempfile::tempdir()?;
        let target = EnginePackageEnumTarget::MacosArm;
        std::fs::create_dir(directory.path().join("bin"))?;
        let path = directory.path().join(target.relative_path());
        let bytes = b"report engine transport fixture";
        std::fs::write(&path, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
        }
        let manifest = EnginePackageManifest::new(EnginePackageEntry::portable(
            target,
            u64::try_from(bytes.len())?,
            PackageDigest::hash_slice(bytes),
            "fixture".to_owned(),
        )?)?;
        let package = EnginePackageSelector::select(
            directory.path(),
            &manifest,
            EnginePackageHost::from_observation(target, "14.0", "", true)?,
        )?;
        let (sender, receiver) = mpsc::channel(64);
        let provider = ReportFixtureProvider {
            receiver: Mutex::new(Some(receiver)),
            sender: sender.clone(),
        };
        let mut engine = EngineSession::start(&provider, package).await?;
        engine
            .configure_resources(EngineResourceConfig::new(1, 16, ClockDuration::ZERO)?)
            .await?;
        Ok(Self {
            directory,
            search: EngineSearchSession::new(engine),
            sender,
            capacity: CapacityBudget::new(NonZeroUsize::new(1).ok_or("Empty CPU budget")?)?,
        })
    }

    pub(super) fn job(&self, moves: &[&str]) -> ReportFixtureResult<ReportJob> {
        self.job_with_id(moves, 20)
    }

    pub(super) fn job_with_id(
        &self,
        moves: &[&str],
        identity: u128,
    ) -> ReportFixtureResult<ReportJob> {
        self.job_with_source(Self::source(&Self::tree(moves)?)?, identity)
    }

    pub(super) fn job_with_source(
        &self,
        source: ReportJobSource,
        identity: u128,
    ) -> ReportFixtureResult<ReportJob> {
        let config = ReportJobConfiguration::capture(
            self.search.engine(),
            ReportJobBudget::default_per_position()?,
            ReportScoringEnumPolicyVersion::ExpectedScoreV1,
        )?;
        Ok(ReportJob::new(
            ReportJobId::try_from(Uuid::from_u128(identity))?,
            source,
            config,
        )?)
    }

    pub(super) fn session(&self) -> &EngineSearchSession<ReportFixtureProcess> {
        &self.search
    }

    pub(super) async fn start(
        &mut self,
        attempt: &ReportJobAttempt,
        limit: Option<EngineSearchLimit>,
    ) -> ReportFixtureResult<EngineSearchTag> {
        self.start_with(
            attempt.context(),
            attempt.position().clone(),
            limit.unwrap_or(attempt.limit()),
        )
        .await
    }

    pub(super) async fn start_with(
        &mut self,
        context: EngineSearchContext,
        position: EngineSearchPosition,
        limit: EngineSearchLimit,
    ) -> ReportFixtureResult<EngineSearchTag> {
        Ok(self
            .search
            .start(EngineSearchSessionRequest::new(
                context,
                position,
                limit,
                self.capacity.try_lease(1)?,
            ))
            .await?)
    }

    pub(super) async fn configure_hash(&mut self, mib: u32) -> ReportFixtureResult {
        self.search
            .idle_engine()?
            .configure_resources(EngineResourceConfig::new(1, mib, ClockDuration::ZERO)?)
            .await?;
        Ok(())
    }

    pub(super) async fn finish(
        &mut self,
        tag: EngineSearchTag,
        info: &str,
    ) -> ReportFixtureResult<EngineSearchSessionTerminal> {
        let position = self
            .search
            .metadata()
            .ok_or("Missing search metadata")?
            .position()
            .current();
        let movement = position
            .legal_moves()
            .first()
            .ok_or("Missing legal move")?
            .uci(position.variant());
        self.sender
            .send(EngineProcessLine::new(
                EngineProcessEnumPipe::Stdout,
                info.to_owned(),
            )?)
            .await?;
        self.sender
            .send(EngineProcessLine::new(
                EngineProcessEnumPipe::Stdout,
                format!("bestmove {movement}"),
            )?)
            .await?;
        for _ in 0..8 {
            if let EngineSearchSessionEnumEvent::Finished(terminal) =
                self.search.next_event(tag).await?
            {
                return Ok(terminal);
            }
        }
        Err("Search fixture did not finish within its event bound".into())
    }

    pub(super) async fn complete(
        &mut self,
        job: &mut ReportJob,
        index: u32,
        info: &str,
    ) -> ReportFixtureResult {
        let attempt = job.start_position(index)?;
        let tag = self.start(&attempt, None).await?;
        let binding = job.bind_engine(&attempt, &self.search)?;
        let terminal = self.finish(tag, info).await?;
        job.record_terminal(&binding, &terminal)?;
        Ok(())
    }

    pub(super) async fn close(mut self) -> ReportFixtureResult {
        self.search
            .shutdown_retained(Instant::now() + std::time::Duration::from_secs(1))
            .await?;
        self.directory.close()?;
        Ok(())
    }
}
