use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use crypto::rust::util::certificate_helper::{CertificateHelper, CertificatePrinter};
use futures::StreamExt;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

use super::*;

fn server() -> F1r3flyServer {
    let (secret, public) = CertificateHelper::generate_key_pair();
    let certificate = CertificateHelper::generate_certificate(&secret, &public).unwrap();
    let certificate = CertificatePrinter::print_certificate(&certificate);
    let key = CertificatePrinter::print_private_key_from_secret(&secret).unwrap();
    F1r3flyServer::builder(
        "accept-resource-test".to_owned(),
        &certificate,
        &key,
        "127.0.0.1:0".parse().unwrap(),
    )
    .unwrap()
}

#[tokio::test(start_paused = true)]
async fn accept_errors_back_off_exponentially_up_to_one_second() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let observed = attempts.clone();
    let errors = futures::stream::iter((0..10).map(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        Err(io::Error::from_raw_os_error(24))
    }));
    let mut incoming = server().incoming_from(errors);
    let started = tokio::time::Instant::now();
    for (index, milliseconds) in [0, 10, 30, 70, 150, 310, 630, 1270, 2270, 3270]
        .into_iter()
        .enumerate()
    {
        assert!(matches!(
            incoming.next().await,
            Some(Err(F1r3flyServerError::Io(e))) if e.raw_os_error() == Some(24)
        ));
        assert_eq!(started.elapsed(), Duration::from_millis(milliseconds));
        assert_eq!(attempts.load(Ordering::SeqCst), index + 1);
    }
    drop(incoming);
    tokio::time::timeout(Duration::from_secs(1), async {
        while Arc::strong_count(&attempts) != 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("dropping incoming must stop its error retry task");
}

#[tokio::test]
async fn successful_accept_resets_backoff_without_delaying_the_next_accept() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let _client = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (connection, _) = listener.accept().await.unwrap();
    let attempts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = attempts.clone();
    let stream = futures::stream::iter([
        Err(io::Error::from_raw_os_error(24)),
        Err(io::Error::from_raw_os_error(24)),
        Ok(connection),
        Err(io::Error::from_raw_os_error(24)),
        Err(io::Error::from_raw_os_error(24)),
    ])
    .inspect(move |_| observed.lock().unwrap().push(tokio::time::Instant::now()));
    tokio::time::pause();
    let started = tokio::time::Instant::now();
    let mut incoming = server().incoming_from(stream);
    for _ in 0..4 {
        assert!(matches!(
            incoming.next().await,
            Some(Err(F1r3flyServerError::Io(e))) if e.raw_os_error() == Some(24)
        ));
    }
    let times: Vec<_> = attempts
        .lock()
        .unwrap()
        .iter()
        .map(|instant| instant.duration_since(started))
        .collect();
    assert_eq!(times, [0, 10, 30, 30, 40].map(Duration::from_millis));
}

#[tokio::test(start_paused = true)]
async fn closing_incoming_interrupts_backoff() {
    let stream = futures::stream::iter([Err(io::Error::from_raw_os_error(24))])
        .chain(futures::stream::pending());
    let mut incoming = server().incoming_from(stream);
    assert!(incoming.next().await.unwrap().is_err());
    incoming.receiver.close();
    tokio::time::timeout(Duration::from_millis(1), &mut incoming._listener_task)
        .await
        .expect("closing incoming must interrupt the retry delay")
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn closing_idle_incoming_stops_listener() {
    let mut incoming = server().incoming_from(futures::stream::pending());
    tokio::task::yield_now().await;
    incoming.receiver.close();
    tokio::time::timeout(Duration::from_millis(1), &mut incoming._listener_task)
        .await
        .expect("closing incoming must stop a pending accept")
        .unwrap();
}

#[derive(Clone, Default)]
struct AcceptEvents(
    Arc<std::sync::Mutex<Vec<(tokio::time::Instant, tracing::Level, Option<u64>)>>>,
);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for AcceptEvents {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        #[derive(Default)]
        struct Suppressed(Option<u64>);

        impl tracing::field::Visit for Suppressed {
            fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
                if field.name() == "suppressed_errors" {
                    self.0 = Some(value);
                }
            }

            fn record_debug(&mut self, _: &tracing::field::Field, _: &dyn std::fmt::Debug) {}
        }

        let mut suppressed = Suppressed::default();
        event.record(&mut suppressed);
        self.0.lock().unwrap().push((
            tokio::time::Instant::now(),
            *event.metadata().level(),
            suppressed.0,
        ));
    }
}

#[tokio::test(start_paused = true)]
async fn repeated_accept_errors_limit_logs_and_summarize_suppressed_errors() {
    use tracing_subscriber::prelude::*;

    let events = AcceptEvents::default();
    let subscriber = tracing_subscriber::registry().with(events.clone());
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let errors = futures::stream::iter((0..130).map(|_| Err(io::Error::from_raw_os_error(24))));
    let mut incoming = server().incoming_from(errors);
    let started = tokio::time::Instant::now();
    for _ in 0..130 {
        assert!(matches!(
            incoming.next().await,
            Some(Err(F1r3flyServerError::Io(e))) if e.raw_os_error() == Some(24)
        ));
    }
    let events = events.0.lock().unwrap();
    let error_times: Vec<_> = events
        .iter()
        .filter_map(|(time, level, _)| (*level == tracing::Level::ERROR).then_some(*time))
        .collect();
    assert_eq!(error_times.len(), 124);
    assert!(error_times
        .windows(2)
        .all(|pair| pair[1] - pair[0] >= Duration::from_secs(1)));
    let summaries: Vec<_> = events
        .iter()
        .filter_map(|(time, _, suppressed)| suppressed.map(|count| (*time, count)))
        .collect();
    assert_eq!(summaries.len(), 2);
    assert!(summaries[0].0 - started >= Duration::from_secs(60));
    assert!(summaries[1].0 - summaries[0].0 >= Duration::from_secs(60));
    assert_eq!(summaries.iter().map(|(_, count)| count).sum::<u64>(), 6);
}

#[tokio::test]
async fn dropping_incoming_releases_listener_and_stalled_handshakes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let observed = accepted.clone();
    let stream = TcpListenerStream::new(listener).inspect(move |result| {
        if result.is_ok() {
            observed.fetch_add(1, Ordering::SeqCst);
        }
    });
    let incoming = server()
        .handshake_timeout(Duration::from_secs(2))
        .incoming_from(stream);
    let mut client = TcpStream::connect(address).await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        while accepted.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    tokio::task::yield_now().await;
    drop(incoming);
    let mut byte = [0];
    let closed = tokio::time::timeout(Duration::from_secs(1), client.read(&mut byte)).await;
    assert!(
        matches!(closed, Ok(Ok(0)) | Ok(Err(_))),
        "a dropped incoming stream must close stalled peers"
    );
    let rebound = TcpListener::bind(address).await;
    assert!(rebound.is_ok(), "the listener socket must be released");
}

#[cfg(target_os = "linux")]
#[test]
fn real_descriptor_exhaustion_recovers_without_a_log_storm() {
    const CHILD: &str = "F1R3_ACCEPT_RESOURCE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let test = format!(
            "{}::real_descriptor_exhaustion_recovers_without_a_log_storm",
            module_path!().strip_prefix("comm::").unwrap()
        );
        let mut child = std::process::Command::new("bash")
            .args(["-c", "ulimit -n 64 && exec \"$@\"", "accept-resource-test"])
            .arg(std::env::current_exe().unwrap())
            .args(["--exact", &test, "--nocapture"])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                let output = child.wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "descriptor child failed: {}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                print!("{}", String::from_utf8_lossy(&output.stdout));
                return;
            }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("descriptor child exceeded its ten-second limit");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    struct CountBytes(Arc<AtomicUsize>);
    impl io::Write for CountBytes {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.fetch_add(bytes.len(), Ordering::SeqCst);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    let bytes = Arc::new(AtomicUsize::new(0));
    let written = bytes.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .without_time()
        .with_ansi(false)
        .with_writer(move || CountBytes(written.clone()))
        .finish();
    let _subscriber = tracing::subscriber::set_default(subscriber);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let _client = TcpStream::connect(address).await.unwrap();
            let server = server().handshake_timeout(Duration::from_millis(20));
            let errors = Arc::new(AtomicUsize::new(0));
            let observed = errors.clone();
            let stream = TcpListenerStream::new(listener).inspect(move |result| {
                if result.is_err() {
                    observed.fetch_add(1, Ordering::SeqCst);
                }
            });
            let mut descriptors = Vec::new();
            let mut exhausted = false;
            for _ in 0..128 {
                match std::fs::File::open("/dev/null") {
                    Ok(file) => descriptors.push(file),
                    Err(error) => {
                        assert_eq!(error.raw_os_error(), Some(24));
                        exhausted = true;
                        break;
                    }
                }
            }
            assert!(exhausted, "the child must reach its descriptor limit");
            let mut incoming = server.incoming_from(stream);
            assert!(matches!(
                incoming.next().await,
                Some(Err(F1r3flyServerError::Io(e))) if e.raw_os_error() == Some(24)
            ));
            let deadline = tokio::time::sleep(Duration::from_millis(500));
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _ = &mut deadline => break,
                    result = incoming.next() => assert!(matches!(
                        result,
                        Some(Err(F1r3flyServerError::Io(e))) if e.raw_os_error() == Some(24)
                    )),
                }
            }
            let accept_errors = errors.load(Ordering::SeqCst);
            assert!((1..=6).contains(&accept_errors), "accept retries must back off");
            let log_bytes = bytes.load(Ordering::SeqCst);
            assert!(log_bytes > 0 && log_bytes < 1024, "fault output must stay below one KiB");
            let held = descriptors.len();
            drop(descriptors);
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(2), incoming.next()).await.unwrap(),
                Some(Err(F1r3flyServerError::Io(e))) if e.kind() == io::ErrorKind::TimedOut
            ));
            assert_eq!(errors.load(Ordering::SeqCst), accept_errors);
            drop(incoming);
            println!("real EMFILE: {held} held descriptors, {accept_errors} accept errors, {log_bytes} log bytes, accept recovered");
        });
}
