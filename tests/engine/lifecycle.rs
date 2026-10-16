use std::future::Future;
use std::num::NonZeroU32;
use std::sync::Mutex;
use std::task::Poll;
use std::time::Duration;

use chess::board::enums::BoardEnumVariant;
use chess::board::position::BoardPosition;
use chess::clock::duration::ClockDuration;
use chess::engine::capabilities::EngineIdentity;
use chess::engine::enums::EngineEnumOption;
use chess::engine::package::selector::EnginePackageSelected;
use chess::engine::process_provider::{
    EngineProcess, EngineProcessEnumPipe, EngineProcessError, EngineProcessLine,
    EngineProcessProvider,
};
use chess::engine::search::enums::{EngineSearchEnumOutput, EngineSearchEnumState};
use chess::engine::search::identity::{
    EngineSearchContext, EngineSearchGameId, EngineSearchRevision, EngineSearchTag,
};
use chess::engine::search::limit::{EngineSearchClock, EngineSearchLimit};
use chess::engine::search::position::EngineSearchPosition;
use chess::engine::search::stop::EngineSearchStop;
use chess::engine::session::{EngineSession, EngineSessionError};
use tokio::sync::mpsc;
use tokio::time::Instant;
use uuid::Uuid;

use crate::engine_package_selection::EnginePackageFixture;

pub(super) struct EngineLifecycleProvider {
    input: mpsc::Sender<EngineProcessLine>,
    output: Mutex<Option<mpsc::Receiver<EngineProcessLine>>>,
    commands: mpsc::Receiver<String>,
    sent: mpsc::Sender<String>,
    stall_stop_write: bool,
}

impl EngineLifecycleProvider {
    pub(super) fn new() -> Self {
        let (input, output) = mpsc::channel(64);
        let (sent, commands) = mpsc::channel(64);
        Self {
            input,
            output: Mutex::new(Some(output)),
            commands,
            sent,
            stall_stop_write: false,
        }
    }

    pub(super) async fn emit(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.input
            .send(EngineProcessLine::new(
                EngineProcessEnumPipe::Stdout,
                text.to_owned(),
            )?)
            .await?;
        Ok(())
    }

    pub(super) async fn expect(&mut self, command: &str) -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            self.commands.recv().await.ok_or("missing command")?,
            command
        );
        Ok(())
    }

    pub(super) fn assert_no_command(&mut self) {
        assert!(matches!(
            self.commands.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
    }

    pub(super) async fn start(
        &mut self,
        fixture: &EnginePackageFixture,
    ) -> Result<EngineSession<EngineLifecycleProcess>, Box<dyn std::error::Error>> {
        let session = EngineSession::start(self, fixture.select()?).await?;
        self.expect("uci").await?;
        self.expect("isready").await?;
        Ok(session)
    }

    fn context(revision: u64) -> Result<EngineSearchContext, Box<dyn std::error::Error>> {
        Ok(EngineSearchContext::new(
            EngineSearchGameId::try_from(Uuid::from_u128(1))?,
            EngineSearchRevision::new(revision),
        ))
    }

    fn position() -> Result<EngineSearchPosition, Box<dyn std::error::Error>> {
        Ok(EngineSearchPosition::new(
            BoardPosition::standard(),
            std::iter::empty::<&str>(),
        )?)
    }

    async fn search(
        &mut self,
        session: &mut EngineSession<EngineLifecycleProcess>,
        revision: u64,
    ) -> Result<chess::engine::search::identity::EngineSearchTag, Box<dyn std::error::Error>> {
        let tag = session
            .start_search(
                Self::context(revision)?,
                Self::position()?,
                EngineSearchLimit::infinite(),
            )
            .await?;
        self.expect("position startpos").await?;
        self.expect("go infinite").await?;
        Ok(tag)
    }

    async fn begin_stop(
        &mut self,
        session: &mut EngineSession<EngineLifecycleProcess>,
        request: EngineSearchStop,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let waiting = session.stop(request);
        tokio::pin!(waiting);
        tokio::select! {
            result = &mut waiting => return Err(format!("stop completed before bestmove: {result:?}").into()),
            command = self.commands.recv() => assert_eq!(command.as_deref(), Some("stop")),
        }
        Ok(())
    }

    async fn interrupt_stop(
        &mut self,
        fixture: &EnginePackageFixture,
    ) -> Result<(EngineSession<EngineLifecycleProcess>, EngineSearchTag), Box<dyn std::error::Error>>
    {
        self.stall_stop_write = true;
        let mut session = self.start(fixture).await?;
        let tag = self.search(&mut session, 1).await?;
        self.begin_stop(
            &mut session,
            EngineSearchStop::received_at(tag, Instant::now())?,
        )
        .await?;
        Ok((session, tag))
    }
}

impl EngineProcessProvider for EngineLifecycleProvider {
    type Process = EngineLifecycleProcess;

    fn spawn(&self, _package: &EnginePackageSelected) -> Result<Self::Process, EngineProcessError> {
        Ok(EngineLifecycleProcess {
            input: self.input.clone(),
            output: self
                .output
                .lock()
                .map_err(|_error| std::io::Error::other("fixture lock poisoned"))?
                .take()
                .ok_or_else(|| std::io::Error::other("fixture already spawned"))?,
            sent: self.sent.clone(),
            stall_stop_write: self.stall_stop_write,
            shutdown_complete: false,
        })
    }
}

pub(super) struct EngineLifecycleProcess {
    input: mpsc::Sender<EngineProcessLine>,
    output: mpsc::Receiver<EngineProcessLine>,
    sent: mpsc::Sender<String>,
    stall_stop_write: bool,
    shutdown_complete: bool,
}

impl EngineLifecycleProcess {
    pub(super) async fn emit(&self, text: String) -> Result<(), EngineProcessError> {
        self.input
            .send(EngineProcessLine::new(EngineProcessEnumPipe::Stdout, text)?)
            .await
            .map_err(|_error| std::io::Error::other("fixture input closed"))?;
        Ok(())
    }
}

impl EngineProcess for EngineLifecycleProcess {
    async fn send(&mut self, command: &str) -> Result<(), EngineProcessError> {
        self.sent
            .send(command.to_owned())
            .await
            .map_err(|_error| std::io::Error::other("fixture command receiver closed"))?;
        if command == "stop" && self.stall_stop_write {
            return std::future::pending().await;
        }
        match command {
            "uci" => {
                self.emit("id name Reckless 0.9.0".to_owned()).await?;
                self.emit(format!("id author {}", EngineIdentity::AUTHOR))
                    .await?;
                for option in EngineEnumOption::ALL {
                    self.emit(format!(
                        "option name {} type {}",
                        option.name(),
                        option.descriptor()
                    ))
                    .await?;
                }
                self.emit("uciok".to_owned()).await?;
            }
            "isready" => self.emit("readyok".to_owned()).await?,
            _ => {}
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<EngineProcessLine, EngineProcessError> {
        self.output
            .recv()
            .await
            .ok_or(EngineProcessError::Closed(EngineProcessEnumPipe::Stdout))
    }

    async fn shutdown(&mut self) -> Result<(), EngineProcessError> {
        self.send("shutdown").await?;
        self.shutdown_complete = true;
        Ok(())
    }

    async fn shutdown_retained(&mut self, deadline: Instant) -> Result<(), EngineProcessError> {
        tokio::time::timeout_at(deadline, self.shutdown())
            .await
            .map_err(EngineProcessError::ShutdownDeadline)?
    }

    fn is_shutdown_complete(&self) -> bool {
        self.shutdown_complete
    }

    async fn terminate(&mut self, _deadline: Instant) -> Result<(), EngineProcessError> {
        self.send("terminate").await?;
        self.shutdown_complete = true;
        Ok(())
    }
}

#[test]
fn tagged_limits_serialize_exclusively_and_reject_upstream_parser_overflow()
-> Result<(), Box<dyn std::error::Error>> {
    let milliseconds = ClockDuration::from_millis;
    let clock = EngineSearchClock::new(
        milliseconds(600_000)?,
        milliseconds(599_123)?,
        milliseconds(5_000)?,
        milliseconds(2_000)?,
        NonZeroU32::new(23),
    )?;
    let cases = [
        (EngineSearchLimit::infinite(), "go infinite"),
        (EngineSearchLimit::depth(12)?, "go depth 12"),
        (EngineSearchLimit::nodes(123_456)?, "go nodes 123456"),
        (
            EngineSearchLimit::move_time(milliseconds(3_000)?)?,
            "go movetime 3000",
        ),
        (
            EngineSearchLimit::clock(clock),
            "go wtime 600000 btime 599123 winc 5000 binc 2000 movestogo 23",
        ),
        (
            EngineSearchLimit::clock(EngineSearchClock::new(
                milliseconds(1)?,
                milliseconds(2)?,
                ClockDuration::ZERO,
                ClockDuration::ZERO,
                None,
            )?),
            "go wtime 1 btime 2 winc 0 binc 0",
        ),
    ];
    for (limit, command) in cases {
        assert_eq!(limit.command(), command);
    }
    assert!(EngineSearchLimit::depth(0).is_err());
    assert!(EngineSearchLimit::depth(128).is_err());
    assert!(EngineSearchLimit::nodes(0).is_err());
    assert!(EngineSearchLimit::nodes(2_147_483_648).is_err());
    assert!(EngineSearchLimit::move_time(ClockDuration::ZERO).is_err());
    assert!(EngineSearchLimit::move_time(ClockDuration::from_nanos(999_999)).is_err());
    assert!(EngineSearchLimit::move_time(milliseconds(2_147_483_648)?).is_err());
    assert!(
        EngineSearchClock::new(
            milliseconds(2_147_483_648)?,
            milliseconds(1)?,
            ClockDuration::ZERO,
            ClockDuration::ZERO,
            None
        )
        .is_err()
    );
    assert!(
        EngineSearchClock::new(
            milliseconds(1)?,
            milliseconds(1)?,
            milliseconds(2_147_483_648)?,
            ClockDuration::ZERO,
            None
        )
        .is_err()
    );
    assert!(
        EngineSearchClock::new(
            milliseconds(1)?,
            milliseconds(1)?,
            ClockDuration::ZERO,
            ClockDuration::ZERO,
            NonZeroU32::new(u32::MAX)
        )
        .is_err()
    );
    assert!(EngineSearchGameId::try_from(Uuid::nil()).is_err());
    Ok(())
}

#[test]
fn positions_preserve_initial_state_repetition_history_and_chess960_encoding()
-> Result<(), Box<dyn std::error::Error>> {
    let history = ["g1f3", "g8f6", "f3g1", "f6g8"];
    let position = EngineSearchPosition::new(BoardPosition::standard(), history)?;
    assert_eq!(
        position.command(),
        "position startpos moves g1f3 g8f6 f3g1 f6g8"
    );
    assert_eq!(
        position.current().identity(),
        BoardPosition::standard().identity()
    );
    assert_ne!(position.current().fen(), BoardPosition::standard().fen());
    let initial =
        BoardPosition::from_fen(&BoardPosition::standard().fen(), BoardEnumVariant::Chess960)?;
    let randomized = EngineSearchPosition::new(initial.clone(), history)?;
    assert_eq!(
        randomized.command(),
        format!("position fen {} moves g1f3 g8f6 f3g1 f6g8", initial.fen())
    );
    let castling = BoardPosition::from_fen(
        "4k3/8/8/8/8/8/8/2K2R2 w F - 0 1",
        BoardEnumVariant::Chess960,
    )?;
    assert!(EngineSearchPosition::new(castling.clone(), ["c1g1"]).is_err());
    let castled = EngineSearchPosition::new(castling, ["c1f1"])?;
    assert!(castled.command().ends_with(" moves c1f1"));
    assert!(EngineSearchPosition::new(BoardPosition::standard(), ["e2e5"]).is_err());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn new_game_readiness_full_history_and_natural_results_are_correlated()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    session.new_game(BoardEnumVariant::Standard).await?;
    provider.expect("ucinewgame").await?;
    provider
        .expect("setoption name UCI_Chess960 value false")
        .await?;
    provider.expect("isready").await?;
    let context = EngineLifecycleProvider::context(7)?;
    let position =
        EngineSearchPosition::new(BoardPosition::standard(), ["g1f3", "g8f6", "f3g1", "f6g8"])?;
    let tag = session
        .start_search(context, position, EngineSearchLimit::nodes(64)?)
        .await?;
    assert_eq!(tag.context(), context);
    assert_eq!(tag.generation(), session.generation());
    provider
        .expect("position startpos moves g1f3 g8f6 f3g1 f6g8")
        .await?;
    provider.expect("go nodes 64").await?;
    provider.emit("readyok").await?;
    let readiness = session.next_event().await?;
    assert_eq!(readiness.tag(), tag);
    assert!(matches!(
        readiness.output(),
        EngineSearchEnumOutput::Readiness
    ));
    assert_eq!(session.state(), EngineSearchEnumState::Searching);
    provider.emit("bestmove e2e4").await?;
    let result = session.next_event().await?;
    assert_eq!(result.tag(), tag);
    assert!(
        matches!(result.output(), EngineSearchEnumOutput::BestMove(movement)
        if movement.uci(BoardEnumVariant::Standard) == "e2e4")
    );
    assert_eq!(session.state(), EngineSearchEnumState::Idle);
    let next = provider.search(&mut session, 8).await?;
    assert_ne!(tag.id(), next.id());
    assert_ne!(tag.context(), next.context());
    session.shutdown().await?;
    provider.expect("stop").await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn active_search_blocks_position_and_configuration_until_final_old_bestmove()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let old = provider.search(&mut session, 1).await?;
    assert!(session.new_game(BoardEnumVariant::Chess960).await.is_err());
    assert!(session.readiness().await.is_err());
    assert!(
        session
            .start_search(
                EngineLifecycleProvider::context(2)?,
                EngineLifecycleProvider::position()?,
                EngineSearchLimit::depth(1)?
            )
            .await
            .is_err()
    );
    provider.assert_no_command();
    provider.emit("info depth 1 score cp 0 pv e2e4").await?;
    provider.emit("readyok").await?;
    provider.emit("bestmove e2e4").await?;
    assert_eq!(
        session
            .stop(EngineSearchStop::received_at(old, Instant::now())?)
            .await?,
        old
    );
    provider.expect("stop").await?;
    assert_eq!(session.state(), EngineSearchEnumState::Idle);
    let new = provider.search(&mut session, 2).await?;
    assert!(matches!(
        session
            .stop(EngineSearchStop::received_at(old, Instant::now())?)
            .await,
        Err(EngineSessionError::StaleSearch)
    ));
    provider.assert_no_command();
    assert_eq!(session.state(), EngineSearchEnumState::Searching);
    provider.emit("bestmove d2d4").await?;
    let result = session.next_event().await?;
    assert_eq!(result.tag(), new);
    assert_ne!(result.tag(), old);
    assert!(
        matches!(result.output(), EngineSearchEnumOutput::BestMove(movement)
        if movement.uci(BoardEnumVariant::Standard) == "d2d4")
    );
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn stop_deadline_includes_queue_delay_and_readyok_cannot_complete_it()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let tag = provider.search(&mut session, 1).await?;
    let receipt = Instant::now();
    let request = EngineSearchStop::received_at(tag, receipt)?;
    tokio::time::advance(Duration::from_millis(1_500)).await;
    provider.emit("readyok").await?;
    let result = session.stop(request).await;
    assert!(matches!(result, Err(EngineSessionError::StopDeadline)));
    assert_eq!(Instant::now() - receipt, Duration::from_secs(2));
    assert_eq!(session.state(), EngineSearchEnumState::Failed);
    provider.expect("stop").await?;
    provider.expect("terminate").await?;
    assert!(session.readiness().await.is_err());
    provider.assert_no_command();
    session.shutdown().await?;
    provider.assert_no_command();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn already_expired_stop_never_grants_a_new_deadline_or_delivers_queued_bestmove()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let tag = provider.search(&mut session, 1).await?;
    let request = EngineSearchStop::received_at(tag, Instant::now())?;
    tokio::time::advance(Duration::from_secs(3)).await;
    provider.emit("bestmove e2e4").await?;
    assert!(matches!(
        session.stop(request).await,
        Err(EngineSessionError::StopDeadline)
    ));
    provider.expect("terminate").await?;
    provider.assert_no_command();
    assert_eq!(session.state(), EngineSearchEnumState::Failed);
    session.shutdown().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn dropping_a_stop_wait_preserves_its_invalidation_and_completion_barrier()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let old = provider.search(&mut session, 1).await?;
    provider
        .begin_stop(
            &mut session,
            EngineSearchStop::received_at(old, Instant::now())?,
        )
        .await?;
    assert_eq!(session.state(), EngineSearchEnumState::Stopping);
    assert!(session.new_game(BoardEnumVariant::Standard).await.is_err());
    provider.emit("bestmove e2e4").await?;
    let event = session.next_event().await?;
    assert_eq!(event.tag(), old);
    assert!(matches!(event.output(), EngineSearchEnumOutput::Stopped));
    assert_eq!(session.state(), EngineSearchEnumState::Idle);
    session.shutdown().await?;
    provider.expect("shutdown").await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn safety_deadline_wins_when_bestmove_and_timeout_are_ready_together()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let mut session = provider.start(&fixture).await?;
    let tag = provider.search(&mut session, 1).await?;
    provider
        .begin_stop(
            &mut session,
            EngineSearchStop::received_at(tag, Instant::now())?,
        )
        .await?;
    let result = {
        let waiting = session.next_event();
        tokio::pin!(waiting);
        let initial =
            std::future::poll_fn(|context| Poll::Ready(waiting.as_mut().poll(context))).await;
        assert!(initial.is_pending());
        tokio::time::advance(Duration::from_secs(2)).await;
        provider.emit("bestmove e2e4").await?;
        waiting.await
    };
    assert!(matches!(result, Err(EngineSessionError::StopDeadline)));
    assert_eq!(session.state(), EngineSearchEnumState::Failed);
    provider.expect("terminate").await?;
    session.shutdown().await?;
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_partial_stop_delivery_never_reopens_the_engine_for_another_search()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let (mut session, _tag) = provider.interrupt_stop(&fixture).await?;
    provider.emit("bestmove e2e4").await?;
    assert!(session.next_event().await.is_err());
    assert_eq!(session.state(), EngineSearchEnumState::Failed);
    provider.expect("terminate").await?;
    assert!(
        session
            .start_search(
                EngineLifecycleProvider::context(2)?,
                EngineLifecycleProvider::position()?,
                EngineSearchLimit::depth(1)?
            )
            .await
            .is_err()
    );
    provider.assert_no_command();
    session.shutdown().await?;
    provider.assert_no_command();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_partial_stop_cannot_be_completed_by_a_second_stop_request()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let (mut session, tag) = provider.interrupt_stop(&fixture).await?;
    provider.emit("bestmove e2e4").await?;
    assert!(
        session
            .stop(EngineSearchStop::received_at(tag, Instant::now())?)
            .await
            .is_err()
    );
    assert_eq!(session.state(), EngineSearchEnumState::Failed);
    provider.expect("terminate").await?;
    session.shutdown().await?;
    provider.assert_no_command();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn cancelled_partial_stop_shutdown_never_appends_protocol_commands()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut provider = EngineLifecycleProvider::new();
    let (session, _tag) = provider.interrupt_stop(&fixture).await?;
    session.shutdown().await?;
    provider.expect("terminate").await?;
    provider.assert_no_command();
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn malformed_or_illegal_bestmoves_fail_and_reap_without_becoming_results()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    for output in [
        "bestmove e2e5",
        "bestmove 0000",
        "bestmove e2e4 ponder e7e5",
        "bestmove",
        "unrecognized output",
    ] {
        let mut provider = EngineLifecycleProvider::new();
        let mut session = provider.start(&fixture).await?;
        provider.search(&mut session, 1).await?;
        provider.emit(output).await?;
        assert!(session.next_event().await.is_err());
        assert_eq!(session.state(), EngineSearchEnumState::Failed);
        provider.expect("terminate").await?;
        session.shutdown().await?;
        provider.assert_no_command();
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn terminal_positions_never_reach_engine_io_and_restarts_get_new_generations()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = EnginePackageFixture::new()?;
    let mut first = EngineLifecycleProvider::new();
    let mut session = first.start(&fixture).await?;
    let terminal =
        BoardPosition::from_fen("7k/6Q1/5K2/8/8/8/8/8 b - - 0 1", BoardEnumVariant::Standard)?;
    assert!(
        session
            .start_search(
                EngineLifecycleProvider::context(1)?,
                EngineSearchPosition::new(terminal, std::iter::empty::<&str>())?,
                EngineSearchLimit::depth(1)?
            )
            .await
            .is_err()
    );
    first.assert_no_command();
    assert_eq!(session.state(), EngineSearchEnumState::Idle);
    let generation = session.generation();
    session.shutdown().await?;
    first.expect("shutdown").await?;
    let mut second = EngineLifecycleProvider::new();
    let session = second.start(&fixture).await?;
    assert_ne!(session.generation(), generation);
    session.shutdown().await?;
    second.expect("shutdown").await?;
    Ok(())
}
