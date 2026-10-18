use std::future::Future;
use std::pin::Pin;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll};

use chess::desktop::identity::DesktopRequestId;
use chess::desktop::profile::DesktopProfileId;
use chess::desktop::profile::activation::server::{
    DesktopProfileActivationAcceptor, DesktopProfileActivationServer,
};
use chess::desktop::profile::activation::{
    DesktopProfileActivationBinding, DesktopProfileActivationCodec,
    DesktopProfileActivationEnumResult, DesktopProfileActivationError,
    DesktopProfileActivationExchange, DesktopProfileActivationPeer,
    DesktopProfileActivationRequest, DesktopProfileActivationResponse,
    DesktopProfileActivationWindow,
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};

struct DesktopProfileActivationAcceptorFixture {
    streams: tokio::sync::mpsc::Receiver<DesktopProfileActivationTransportFixture>,
    accepted: tokio::sync::mpsc::Sender<()>,
}

impl DesktopProfileActivationAcceptor for DesktopProfileActivationAcceptorFixture {
    type Stream = DesktopProfileActivationTransportFixture;

    async fn accept(&mut self) -> Result<Self::Stream, DesktopProfileActivationError> {
        let stream = self.streams.recv().await.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "The fixture listener closed",
            )
        })?;
        self.accepted.try_send(()).map_err(std::io::Error::other)?;
        Ok(stream)
    }
}

#[tokio::test(start_paused = true)]
async fn server_reserves_capacity_before_accept_and_drains_handlers_on_shutdown()
-> Result<(), Box<dyn std::error::Error>> {
    let count = DesktopProfileActivationExchange::CLIENT_CAPACITY;
    let (streams, receiver) = tokio::sync::mpsc::channel(count + 1);
    let (accepted, mut accepted_events) = tokio::sync::mpsc::channel(count + 1);
    let (shutdown, shutdown_events) = tokio::sync::watch::channel(false);
    let window = Arc::new(DesktopProfileActivationWindowFixture::default());
    let mut clients = Vec::new();
    for _ in 0..=count {
        let (owner, client) = tokio::io::duplex(8192);
        streams
            .send(DesktopProfileActivationTransportFixture::new(owner))
            .await?;
        clients.push(client);
    }
    let binding = DesktopProfileActivationFixture::binding()?;
    let owner_window = Arc::clone(&window);
    let server = tokio::spawn(async move {
        let mut listener = DesktopProfileActivationAcceptorFixture {
            streams: receiver,
            accepted,
        };
        DesktopProfileActivationServer::run(&mut listener, binding, owner_window, shutdown_events)
            .await
    });
    for _ in 0..count {
        accepted_events
            .recv()
            .await
            .ok_or_else(|| std::io::Error::other("Expected an accepted fixture connection"))?;
    }
    shutdown.send(false)?;
    tokio::task::yield_now().await;
    assert!(!server.is_finished());
    assert!(matches!(
        accepted_events.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
    shutdown.send(true)?;
    let report = server.await??;
    assert_eq!(report.accepted(), u64::try_from(count)?);
    assert_eq!(report.completed(), 0);
    assert_eq!(report.refused(), u64::try_from(count)?);
    assert!(matches!(
        report.last_refusal(),
        Some(DesktopProfileActivationError::Deadline)
    ));
    assert_eq!(window.notifications.load(Ordering::Relaxed), 0);
    drop(clients);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_invalid_peer_does_not_stop_other_activation_clients()
-> Result<(), Box<dyn std::error::Error>> {
    let (streams, receiver) = tokio::sync::mpsc::channel(2);
    let (accepted, mut accepted_events) = tokio::sync::mpsc::channel(2);
    let (shutdown, shutdown_events) = tokio::sync::watch::channel(false);
    let window = Arc::new(DesktopProfileActivationWindowFixture::default());
    let (owner, secondary) = tokio::io::duplex(8192);
    streams
        .send(DesktopProfileActivationTransportFixture::new(owner))
        .await?;
    let (untrusted_owner, _untrusted_client) = tokio::io::duplex(8192);
    let mut untrusted = DesktopProfileActivationTransportFixture::new(untrusted_owner);
    untrusted.is_authenticated = false;
    streams.send(untrusted).await?;
    let request = DesktopProfileActivationFixture::request()?;
    let owner_window = Arc::clone(&window);
    let server = tokio::spawn(async move {
        let mut listener = DesktopProfileActivationAcceptorFixture {
            streams: receiver,
            accepted,
        };
        DesktopProfileActivationServer::run(
            &mut listener,
            request.binding(),
            owner_window,
            shutdown_events,
        )
        .await
    });
    for _ in 0..2 {
        accepted_events
            .recv()
            .await
            .ok_or_else(|| std::io::Error::other("Expected an accepted fixture connection"))?;
    }
    let result = DesktopProfileActivationExchange::activate(
        DesktopProfileActivationTransportFixture::new(secondary),
        request,
        tokio::time::Instant::now() + DesktopProfileActivationExchange::CLIENT_DEADLINE,
    )
    .await?;
    assert_eq!(result, DesktopProfileActivationEnumResult::OwnerStarting);
    shutdown.send(true)?;
    let report = server.await??;
    assert_eq!(report.accepted(), 2);
    assert_eq!(report.completed(), 1);
    assert_eq!(report.refused(), 1);
    assert!(matches!(
        report.last_refusal(),
        Some(DesktopProfileActivationError::Unauthenticated)
    ));
    assert_eq!(window.notifications.load(Ordering::Relaxed), 1);
    Ok(())
}

struct DesktopProfileActivationFixture;

impl DesktopProfileActivationFixture {
    const VALID_REQUEST_JSON: &[u8] = br#"{"version":2,"identity":{"profile_id":"61606ef1-e259-42ce-8510-aa380a8f739e","generation":"f135d9c6-f0a4-4c69-bc18-a042a86b105d","request_id":"d129d71c-ed98-41aa-a71c-9840659bd468"},"message":{"operation":"activate"}}"#;

    fn binding() -> Result<DesktopProfileActivationBinding, Box<dyn std::error::Error>> {
        Ok(DesktopProfileActivationBinding::new(
            DesktopProfileId::from_str("61606ef1-e259-42ce-8510-aa380a8f739e")?,
            DesktopRequestId::from_str("f135d9c6-f0a4-4c69-bc18-a042a86b105d")?,
        ))
    }

    fn request() -> Result<DesktopProfileActivationRequest, Box<dyn std::error::Error>> {
        Ok(DesktopProfileActivationRequest::new(
            Self::binding()?,
            DesktopRequestId::from_str("d129d71c-ed98-41aa-a71c-9840659bd468")?,
        ))
    }

    async fn decode(
        body: &[u8],
    ) -> Result<DesktopProfileActivationRequest, DesktopProfileActivationError> {
        let (mut output, mut input) = tokio::io::duplex(8192);
        output.write_u32(u32::try_from(body.len())?).await?;
        output.write_all(body).await?;
        drop(output);
        DesktopProfileActivationCodec::read_request(&mut input).await
    }

    async fn respond_and_consume_receipt(
        owner: &mut DuplexStream,
        request: DesktopProfileActivationRequest,
    ) -> Result<DesktopProfileActivationResponse, DesktopProfileActivationError> {
        assert_eq!(
            DesktopProfileActivationCodec::read_request(owner).await?,
            request
        );
        let response = DesktopProfileActivationResponse::new(
            request,
            DesktopProfileActivationEnumResult::ActivationRequested,
        );
        DesktopProfileActivationCodec::write_response(owner, response).await?;
        assert_eq!(
            DesktopProfileActivationCodec::read_receipt(owner).await?,
            request
        );
        Ok(response)
    }
}

#[tokio::test]
async fn activation_frame_round_trip_preserves_the_complete_binding()
-> Result<(), Box<dyn std::error::Error>> {
    let expected = DesktopProfileActivationFixture::request()?;
    let (mut output, mut input) = tokio::io::duplex(8192);
    DesktopProfileActivationCodec::write_request(&mut output, &expected).await?;
    drop(output);
    let decoded = DesktopProfileActivationCodec::read_request(&mut input).await?;
    assert_eq!(decoded, expected);
    assert_eq!(
        decoded.binding(),
        DesktopProfileActivationFixture::binding()?
    );
    Ok(())
}

#[tokio::test]
async fn activation_lengths_are_checked_before_allocating_or_decoding()
-> Result<(), Box<dyn std::error::Error>> {
    for length in [0, 4097, u32::MAX] {
        let (mut output, mut input) = tokio::io::duplex(16);
        output.write_u32(length).await?;
        drop(output);
        assert!(matches!(
            DesktopProfileActivationCodec::read_request(&mut input).await,
            Err(DesktopProfileActivationError::FrameLength { received }) if received == length
        ));
    }
    assert!(matches!(
        DesktopProfileActivationFixture::decode(&[b' '; 4096]).await,
        Err(DesktopProfileActivationError::Json(_))
    ));
    Ok(())
}

#[tokio::test]
async fn truncated_headers_and_payloads_fail_without_a_partial_request()
-> Result<(), Box<dyn std::error::Error>> {
    for bytes in [vec![0, 0, 0], vec![0, 0, 0, 8, b'{']] {
        let (mut output, mut input) = tokio::io::duplex(32);
        output.write_all(&bytes).await?;
        drop(output);
        assert!(matches!(
            DesktopProfileActivationCodec::read_request(&mut input).await,
            Err(DesktopProfileActivationError::Io(_))
        ));
    }
    Ok(())
}

#[tokio::test]
async fn activation_rejects_unknown_fields_commands_versions_and_noncanonical_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let valid = DesktopProfileActivationFixture::VALID_REQUEST_JSON;
    let text = std::str::from_utf8(valid)?;
    for invalid in [
        text.replace("\"operation\":\"activate\"", "\"operation\":\"resume\""),
        text.replace("\"version\":2", "\"version\":1"),
        text.replace("\"version\":2", "\"version\":3"),
        text.replace("\"version\":2", "\"version\":2,\"path\":\"private\""),
        text.replace(
            "61606ef1-e259-42ce-8510-aa380a8f739e",
            "00000000-0000-0000-0000-000000000000",
        ),
        text.replace(
            "f135d9c6-f0a4-4c69-bc18-a042a86b105d",
            "F135D9C6-F0A4-4C69-BC18-A042A86B105D",
        ),
        text.replace("d129d71c-ed98-41aa-a71c-9840659bd468", "invalid"),
    ] {
        assert!(
            DesktopProfileActivationFixture::decode(invalid.as_bytes())
                .await
                .is_err()
        );
    }
    assert!(DesktopProfileActivationFixture::decode(valid).await.is_ok());
    Ok(())
}

struct DesktopProfileActivationTransportFixture {
    stream: DuplexStream,
    is_authenticated: bool,
    reads: Arc<AtomicUsize>,
    read_delay: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl DesktopProfileActivationTransportFixture {
    fn new(stream: DuplexStream) -> Self {
        Self {
            stream,
            is_authenticated: true,
            reads: Arc::new(AtomicUsize::new(0)),
            read_delay: None,
        }
    }
}

impl DesktopProfileActivationPeer for DesktopProfileActivationTransportFixture {
    fn authenticate(
        &self,
    ) -> impl std::future::Future<Output = Result<(), DesktopProfileActivationError>> + Send {
        std::future::ready(if self.is_authenticated {
            Ok(())
        } else {
            Err(DesktopProfileActivationError::Unauthenticated)
        })
    }
}

impl AsyncRead for DesktopProfileActivationTransportFixture {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        this.reads.fetch_add(1, Ordering::Relaxed);
        if let Some(delay) = &mut this.read_delay {
            match delay.as_mut().poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(()) => this.read_delay = None,
            }
        }
        Pin::new(&mut this.stream).poll_read(cx, buffer)
    }
}

impl AsyncWrite for DesktopProfileActivationTransportFixture {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, bytes)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

#[derive(Default)]
struct DesktopProfileActivationWindowFixture {
    notifications: AtomicUsize,
}

impl DesktopProfileActivationWindow for DesktopProfileActivationWindowFixture {
    fn request_activation(
        &self,
    ) -> Result<DesktopProfileActivationEnumResult, DesktopProfileActivationError> {
        self.notifications.fetch_add(1, Ordering::Relaxed);
        Ok(DesktopProfileActivationEnumResult::OwnerStarting)
    }
}

#[tokio::test(start_paused = true)]
async fn one_authenticated_exchange_requires_a_matching_consumption_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let window = DesktopProfileActivationWindowFixture::default();
    let (owner, secondary) = tokio::io::duplex(8192);
    let now = tokio::time::Instant::now();
    let (server, client) = tokio::join!(
        DesktopProfileActivationExchange::serve(
            DesktopProfileActivationTransportFixture::new(owner),
            request.binding(),
            &window,
            now
        ),
        DesktopProfileActivationExchange::activate(
            DesktopProfileActivationTransportFixture::new(secondary),
            request,
            now + DesktopProfileActivationExchange::CLIENT_DEADLINE
        ),
    );
    assert_eq!(server?, DesktopProfileActivationEnumResult::OwnerStarting);
    assert_eq!(client?, DesktopProfileActivationEnumResult::OwnerStarting);
    assert_eq!(window.notifications.load(Ordering::Relaxed), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_unauthenticated_peer_cannot_send_a_trusted_frame_or_activate()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let window = DesktopProfileActivationWindowFixture::default();
    let (owner, mut secondary) = tokio::io::duplex(8192);
    let mut transport = DesktopProfileActivationTransportFixture::new(owner);
    transport.is_authenticated = false;
    let reads = Arc::clone(&transport.reads);
    DesktopProfileActivationCodec::write_request(&mut secondary, &request).await?;
    assert!(matches!(
        DesktopProfileActivationExchange::serve(
            transport,
            request.binding(),
            &window,
            tokio::time::Instant::now()
        )
        .await,
        Err(DesktopProfileActivationError::Unauthenticated)
    ));
    assert_eq!(reads.load(Ordering::Relaxed), 0);
    assert_eq!(window.notifications.load(Ordering::Relaxed), 0);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn queued_connections_cannot_reset_their_accept_deadline()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let window = DesktopProfileActivationWindowFixture::default();
    let accepted = tokio::time::Instant::now();
    tokio::time::advance(DesktopProfileActivationExchange::CONNECTION_DEADLINE).await;
    let (owner, _secondary) = tokio::io::duplex(8192);
    assert!(matches!(
        DesktopProfileActivationExchange::serve(
            DesktopProfileActivationTransportFixture::new(owner),
            request.binding(),
            &window,
            accepted
        )
        .await,
        Err(DesktopProfileActivationError::Deadline)
    ));
    assert_eq!(window.notifications.load(Ordering::Relaxed), 0);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_missing_receipt_expires_the_entire_exchange() -> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let window = Arc::new(DesktopProfileActivationWindowFixture::default());
    let observed = Arc::clone(&window);
    let (owner, mut secondary) = tokio::io::duplex(8192);
    let accepted = tokio::time::Instant::now();
    let server = tokio::spawn(async move {
        DesktopProfileActivationExchange::serve(
            DesktopProfileActivationTransportFixture::new(owner),
            request.binding(),
            window.as_ref(),
            accepted,
        )
        .await
    });
    DesktopProfileActivationCodec::write_request(&mut secondary, &request).await?;
    let response = DesktopProfileActivationCodec::read_response(&mut secondary).await?;
    response.check_request(request)?;
    assert!(matches!(
        server.await?,
        Err(DesktopProfileActivationError::Deadline)
    ));
    assert_eq!(
        tokio::time::Instant::now() - accepted,
        DesktopProfileActivationExchange::CONNECTION_DEADLINE
    );
    assert_eq!(observed.notifications.load(Ordering::Relaxed), 1);
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn activation_requires_the_owner_to_acknowledge_its_consumption_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let (mut owner, secondary) = tokio::io::duplex(8192);
    let peer = async {
        DesktopProfileActivationFixture::respond_and_consume_receipt(&mut owner, request).await?;
        drop(owner);
        Ok::<(), DesktopProfileActivationError>(())
    };
    let deadline = tokio::time::Instant::now() + DesktopProfileActivationExchange::CLIENT_DEADLINE;
    let (client, peer) = tokio::join!(
        DesktopProfileActivationExchange::activate(
            DesktopProfileActivationTransportFixture::new(secondary),
            request,
            deadline,
        ),
        peer,
    );
    peer?;
    assert!(matches!(client, Err(DesktopProfileActivationError::Io(_))));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_acknowledgment_must_match_profile_generation_and_request_identity()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let different_id = DesktopRequestId::from_str("4a4c72a6-7cf7-49fe-8c76-25091517f799")?;
    let different_profile = DesktopProfileId::from_str("448987ef-284d-4cdb-8fb6-b48e8a9c9e83")?;
    for acknowledged in [
        DesktopProfileActivationRequest::new(request.binding(), different_id),
        DesktopProfileActivationRequest::new(
            DesktopProfileActivationBinding::new(request.binding().profile(), different_id),
            request.id(),
        ),
        DesktopProfileActivationRequest::new(
            DesktopProfileActivationBinding::new(different_profile, request.binding().generation()),
            request.id(),
        ),
    ] {
        let (mut owner, secondary) = tokio::io::duplex(8192);
        let peer = async {
            DesktopProfileActivationFixture::respond_and_consume_receipt(&mut owner, request)
                .await?;
            DesktopProfileActivationCodec::write_receipt_acknowledged(&mut owner, acknowledged)
                .await?;
            DesktopProfileActivationCodec::require_end(&mut owner).await
        };
        let deadline =
            tokio::time::Instant::now() + DesktopProfileActivationExchange::CLIENT_DEADLINE;
        let (client, peer) = tokio::join!(
            DesktopProfileActivationExchange::activate(
                DesktopProfileActivationTransportFixture::new(secondary),
                request,
                deadline,
            ),
            peer,
        );
        peer?;
        assert!(matches!(
            client,
            Err(DesktopProfileActivationError::BindingMismatch)
        ));
    }
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_response_cannot_replace_the_receipt_acknowledgment()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let (mut owner, secondary) = tokio::io::duplex(8192);
    let peer = async {
        let response =
            DesktopProfileActivationFixture::respond_and_consume_receipt(&mut owner, request)
                .await?;
        DesktopProfileActivationCodec::write_response(&mut owner, response).await?;
        DesktopProfileActivationCodec::require_end(&mut owner).await
    };
    let deadline = tokio::time::Instant::now() + DesktopProfileActivationExchange::CLIENT_DEADLINE;
    let (client, peer) = tokio::join!(
        DesktopProfileActivationExchange::activate(
            DesktopProfileActivationTransportFixture::new(secondary),
            request,
            deadline,
        ),
        peer,
    );
    peer?;
    assert!(matches!(
        client,
        Err(DesktopProfileActivationError::UnexpectedMessage)
    ));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn an_acknowledgment_ready_at_the_original_deadline_cannot_complete_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let (mut owner, secondary) = tokio::io::duplex(8192);
    let started = tokio::time::Instant::now();
    let deadline = started + DesktopProfileActivationExchange::CLIENT_DEADLINE;
    let peer = async {
        DesktopProfileActivationFixture::respond_and_consume_receipt(&mut owner, request).await?;
        tokio::time::sleep_until(deadline).await;
        DesktopProfileActivationCodec::write_receipt_acknowledged(&mut owner, request).await?;
        drop(owner);
        Ok::<(), DesktopProfileActivationError>(())
    };
    let (peer, client) = tokio::join!(
        biased;
        peer,
        DesktopProfileActivationExchange::activate(
            DesktopProfileActivationTransportFixture::new(secondary),
            request,
            deadline,
        ),
    );
    peer?;
    assert!(matches!(
        client,
        Err(DesktopProfileActivationError::Deadline)
    ));
    assert_eq!(
        tokio::time::Instant::now() - started,
        DesktopProfileActivationExchange::CLIENT_DEADLINE,
    );
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn protocol_one_is_rejected_before_window_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let window = DesktopProfileActivationWindowFixture::default();
    let (owner, mut secondary) = tokio::io::duplex(8192);
    let body = std::str::from_utf8(DesktopProfileActivationFixture::VALID_REQUEST_JSON)?
        .replace("\"version\":2", "\"version\":1");
    secondary.write_u32(u32::try_from(body.len())?).await?;
    secondary.write_all(body.as_bytes()).await?;
    drop(secondary);
    assert!(matches!(
        DesktopProfileActivationExchange::serve(
            DesktopProfileActivationTransportFixture::new(owner),
            request.binding(),
            &window,
            tokio::time::Instant::now(),
        )
        .await,
        Err(DesktopProfileActivationError::Version(1))
    ));
    assert_eq!(window.notifications.load(Ordering::Relaxed), 0);
    Ok(())
}

#[tokio::test]
async fn a_receipt_with_additional_bytes_cannot_complete_the_exchange()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let window = DesktopProfileActivationWindowFixture::default();
    let (owner, mut secondary) = tokio::io::duplex(8192);
    let client = async {
        DesktopProfileActivationCodec::write_request(&mut secondary, &request).await?;
        let response = DesktopProfileActivationCodec::read_response(&mut secondary).await?;
        DesktopProfileActivationCodec::write_receipt(&mut secondary, response).await?;
        secondary.write_u8(1).await?;
        assert_eq!(
            DesktopProfileActivationCodec::read_receipt_acknowledged(&mut secondary).await?,
            request,
        );
        drop(secondary);
        Ok::<(), DesktopProfileActivationError>(())
    };
    let (server, client) = tokio::join!(
        DesktopProfileActivationExchange::serve(
            DesktopProfileActivationTransportFixture::new(owner),
            request.binding(),
            &window,
            tokio::time::Instant::now()
        ),
        client,
    );
    client?;
    assert!(matches!(
        server,
        Err(DesktopProfileActivationError::ExtraFrame)
    ));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn a_response_ready_at_the_deadline_cannot_complete_activation()
-> Result<(), Box<dyn std::error::Error>> {
    let request = DesktopProfileActivationFixture::request()?;
    let (mut owner, secondary) = tokio::io::duplex(8192);
    DesktopProfileActivationCodec::write_response(
        &mut owner,
        DesktopProfileActivationResponse::new(
            request,
            DesktopProfileActivationEnumResult::OwnerStarting,
        ),
    )
    .await?;
    let deadline = tokio::time::Instant::now() + DesktopProfileActivationExchange::CLIENT_DEADLINE;
    let mut transport = DesktopProfileActivationTransportFixture::new(secondary);
    transport.read_delay = Some(Box::pin(tokio::time::sleep_until(deadline)));
    assert!(matches!(
        DesktopProfileActivationExchange::activate(transport, request, deadline).await,
        Err(DesktopProfileActivationError::Deadline)
    ));
    drop(owner);
    Ok(())
}
