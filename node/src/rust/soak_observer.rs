use tokio::task::JoinHandle;

use crate::rust::configuration::model::SoakObserverConfig;
use crate::rust::configuration::NodeConf;

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_CONFIG_BYTES: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum ObserverError {
    #[error("The observer requires Linux.")]
    UnsupportedPlatform,
    #[error("The observer configuration is invalid.")]
    Configuration,
    #[error("The observer directory is unsafe.")]
    UnsafeDirectory,
    #[error("The observer peer identity differs.")]
    PeerIdentity,
    #[error("The observer request is invalid.")]
    Request,
    #[error("The observer resource limit was reached.")]
    ResourceLimit,
    #[error("The observer I/O operation failed.")]
    Io(#[from] std::io::Error),
}

pub fn validate_config(config: &SoakObserverConfig) -> Result<(), ObserverError> {
    if !cfg!(target_os = "linux") {
        return Err(ObserverError::UnsupportedPlatform);
    }
    let hexadecimal = |value: &str, length: usize| {
        value.len() == length
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if !config.directory.is_absolute()
        || config.directory.as_os_str().len() > 90
        || config.directory.components().any(|c| {
            !matches!(
                c,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
        || !hexadecimal(&config.source_revision, 40)
        || !hexadecimal(&config.approved_request_sha256, 64)
        || config.peer_pid == 0
        || config.peer_pid > i32::MAX as u32
        || config.peer_start_ticks == 0
        || !(50..=30_000).contains(&config.session_timeout_ms)
        || !(1..=4096).contains(&config.max_sessions)
    {
        return Err(ObserverError::Configuration);
    }
    Ok(())
}

pub struct RunningObserver {
    task: Option<JoinHandle<()>>,
}

impl RunningObserver {
    pub async fn stop(mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

impl Drop for RunningObserver {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

impl Observer {
    pub fn spawn(self) -> RunningObserver {
        RunningObserver {
            task: Some(tokio::spawn(async move {
                if self.run().await.is_err() {
                    tracing::warn!("The local soak observer stopped. Node operation continues.");
                }
            })),
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub struct Observer;

#[cfg(not(target_os = "linux"))]
impl Observer {
    pub fn bind(node: &NodeConf) -> Result<Option<Self>, ObserverError> {
        if node.soak_observer.is_some() {
            Err(ObserverError::UnsupportedPlatform)
        } else {
            Ok(None)
        }
    }

    pub fn authority_handle(
        &self,
    ) -> Option<std::sync::Arc<casper::rust::soak_observer::ObserverController>> {
        None
    }

    pub async fn run(self) -> Result<(), ObserverError> { Err(ObserverError::UnsupportedPlatform) }
}

#[cfg(target_os = "linux")]
pub use linux::{process_start_ticks, Observer};

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::{self, File};
    use std::io::Read;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use crypto::rust::hash::sha_256::Sha256Hasher;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};
    use uuid::Uuid;

    use super::*;

    const MAX_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;

    fn sha256(bytes: Vec<u8>) -> String { hex::encode(Sha256Hasher::hash(bytes)) }

    pub fn process_start_ticks(pid: u32) -> Result<u64, ObserverError> {
        let mut bytes = Vec::new();
        File::open(format!("/proc/{pid}/stat"))?
            .take(4097)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 4096 {
            return Err(ObserverError::ResourceLimit);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| ObserverError::PeerIdentity)?;
        let (_, fields) = text.rsplit_once(") ").ok_or(ObserverError::PeerIdentity)?;
        fields
            .split_whitespace()
            .nth(19)
            .ok_or(ObserverError::PeerIdentity)?
            .parse()
            .map_err(|_| ObserverError::PeerIdentity)
    }

    fn safe_directory(path: &Path, uid: u32) -> Result<(), ObserverError> {
        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component);
            let metadata = fs::symlink_metadata(&current)?;
            if !metadata.is_dir() || (metadata.uid() != 0 && metadata.uid() != uid) {
                return Err(ObserverError::UnsafeDirectory);
            }
            if metadata.mode() & 0o022 != 0
                && !(metadata.uid() == 0 && metadata.mode() & 0o1000 != 0)
            {
                return Err(ObserverError::UnsafeDirectory);
            }
        }
        let metadata = fs::symlink_metadata(path)?;
        if metadata.uid() != uid || metadata.mode() & 0o7777 != 0o700 {
            return Err(ObserverError::UnsafeDirectory);
        }
        Ok(())
    }

    struct SocketGuard {
        path: PathBuf,
        device: u64,
        inode: u64,
    }

    impl Drop for SocketGuard {
        fn drop(&mut self) {
            if let Ok(metadata) = fs::symlink_metadata(&self.path) {
                if metadata.file_type().is_socket()
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode
                    && fs::remove_file(&self.path).is_err()
                {
                    tracing::warn!("The local soak observer socket could not be removed.");
                }
            }
        }
    }

    #[derive(Clone, Serialize)]
    struct Identity {
        schema_version: u32,
        incarnation: String,
        pid: u32,
        process_start_ticks: u64,
        declared_source_revision: String,
        executable_sha256: String,
        configuration_sha256: String,
        configuration_scope: &'static str,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        schema_version: u32,
        request_id: String,
        incarnation: String,
        challenge: String,
        executable_sha256: String,
        configuration_sha256: String,
        approved_request_sha256: String,
        operation: Operation,
        authority: Option<casper::rust::soak_observer::evaluation::AuthorityRequest>,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Operation {
        Capabilities,
        AuthoritySnapshot,
    }

    pub struct Observer {
        listener: UnixListener,
        _socket: SocketGuard,
        config: SoakObserverConfig,
        identity: Identity,
        owner_uid: u32,
        started: Instant,
        sequence: u64,
        controller: std::sync::Arc<casper::rust::soak_observer::ObserverController>,
    }

    impl Drop for Observer {
        fn drop(&mut self) { self.controller.close(); }
    }

    impl Observer {
        pub fn authority_handle(
            &self,
        ) -> Option<std::sync::Arc<casper::rust::soak_observer::ObserverController>> {
            Some(self.controller.clone())
        }

        pub fn bind(node: &NodeConf) -> Result<Option<Self>, ObserverError> {
            let Some(config) = &node.soak_observer else {
                return Ok(None);
            };
            validate_config(config)?;
            let owner_uid = fs::metadata("/proc/self")?.uid();
            safe_directory(&config.directory, owner_uid)?;
            if fs::metadata(format!("/proc/{}", config.peer_pid))?.uid() != owner_uid
                || process_start_ticks(config.peer_pid)? != config.peer_start_ticks
            {
                return Err(ObserverError::PeerIdentity);
            }
            if node.protocol_server.network_id.len() > 256 || node.casper.shard_name.len() > 256 {
                return Err(ObserverError::ResourceLimit);
            }
            let public = json!({
                "schema_version": 1,
                "network_id": node.protocol_server.network_id,
                "shard_name": node.casper.shard_name,
                "standalone": node.standalone,
                "max_parent_depth": node.casper.max_parent_depth,
                "max_number_of_parents": node.casper.max_number_of_parents,
                "fault_tolerance_threshold_bits": node.casper.fault_tolerance_threshold.to_bits()
            });
            let executable = File::open("/proc/self/exe")?;
            if !executable.metadata()?.is_file()
                || executable.metadata()?.len() > MAX_EXECUTABLE_BYTES
            {
                return Err(ObserverError::ResourceLimit);
            }
            let mut bytes = Vec::new();
            executable
                .take(MAX_EXECUTABLE_BYTES + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > MAX_EXECUTABLE_BYTES {
                return Err(ObserverError::ResourceLimit);
            }
            let identity = Identity {
                schema_version: 1,
                incarnation: Uuid::new_v4().to_string(),
                pid: std::process::id(),
                process_start_ticks: process_start_ticks(std::process::id())?,
                declared_source_revision: config.source_revision.clone(),
                executable_sha256: sha256(bytes),
                configuration_sha256: sha256(
                    serde_json::to_vec(&public).map_err(|_| ObserverError::Configuration)?,
                ),
                configuration_scope: "batch-a-public-config-v1",
            };
            let path = config.directory.join("observer.sock");
            let listener = UnixListener::bind(&path)?;
            let metadata = fs::symlink_metadata(&path)?;
            let socket = SocketGuard {
                path: path.clone(),
                device: metadata.dev(),
                inode: metadata.ino(),
            };
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
            let controller =
                casper::rust::soak_observer::ObserverController::new(identity.incarnation.clone());
            Ok(Some(Self {
                controller,
                listener,
                _socket: socket,
                config: config.clone(),
                identity,
                owner_uid,
                started: Instant::now(),
                sequence: 0,
            }))
        }

        fn event(&mut self) -> Result<(u64, u64), ObserverError> {
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or(ObserverError::ResourceLimit)?;
            let elapsed = self
                .started
                .elapsed()
                .as_nanos()
                .try_into()
                .map_err(|_| ObserverError::ResourceLimit)?;
            Ok((self.sequence, elapsed))
        }

        async fn write(
            stream: &mut UnixStream,
            value: &serde_json::Value,
            deadline: tokio::time::Instant,
        ) -> Result<(), ObserverError> {
            if tokio::time::Instant::now() >= deadline {
                return Err(ObserverError::ResourceLimit);
            }
            let bytes = serde_json::to_vec(value).map_err(|_| ObserverError::Request)?;
            if bytes.len() > MAX_FRAME_BYTES {
                return Err(ObserverError::ResourceLimit);
            }
            stream.write_u32(bytes.len() as u32).await?;
            stream.write_all(&bytes).await?;
            Ok(())
        }

        async fn session(
            &mut self,
            mut stream: UnixStream,
            deadline: tokio::time::Instant,
            nonce: impl FnOnce() -> Uuid,
        ) -> Result<(), ObserverError> {
            let peer = stream.peer_cred()?;
            if peer.uid() != self.owner_uid
                || peer.pid() != Some(self.config.peer_pid as i32)
                || process_start_ticks(self.config.peer_pid)? != self.config.peer_start_ticks
            {
                return Err(ObserverError::PeerIdentity);
            }
            let (sequence, monotonic_ns) = self.event()?;
            let challenge = format!("{}:{sequence}", nonce());
            Self::write(&mut stream, &json!({
                "kind": "hello", "identity": self.identity, "challenge": challenge,
                "approved_request_sha256": self.config.approved_request_sha256,
                "permission": "read-only", "max_frame_bytes": MAX_FRAME_BYTES,
                "sequence": sequence, "monotonic_ns": monotonic_ns, "clock": "observer-monotonic"
            }), deadline).await?;
            let size = stream.read_u32().await? as usize;
            if size == 0 || size > MAX_FRAME_BYTES {
                return Err(ObserverError::ResourceLimit);
            }
            let mut bytes = vec![0; size];
            stream.read_exact(&mut bytes).await?;
            let request: Request =
                serde_json::from_slice(&bytes).map_err(|_| ObserverError::Request)?;
            if request.schema_version != 1
                || request.incarnation != self.identity.incarnation
                || request.challenge != challenge
                || request.executable_sha256 != self.identity.executable_sha256
                || request.configuration_sha256 != self.identity.configuration_sha256
                || request.approved_request_sha256 != self.config.approved_request_sha256
                || !Uuid::parse_str(&request.request_id)
                    .is_ok_and(|id| id.to_string() == request.request_id)
            {
                return Err(ObserverError::Request);
            }
            match request.operation {
                Operation::Capabilities => {
                    let (sequence, monotonic_ns) = self.event()?;
                    if request.authority.is_some() {
                        return Err(ObserverError::Request);
                    }
                    let status = self.controller.status();
                    let capabilities: Vec<_> = [
                        "authority",
                        "publication",
                        "durable_work",
                        "fault_control",
                        "recovery",
                    ]
                    .into_iter()
                    .map(|name| {
                        if name == "authority" {
                            json!({"name":name,"supported":status == "attached","reason":status})
                        } else {
                            json!({"name":name,"supported":false,"reason":"not_implemented"})
                        }
                    })
                    .collect();
                    Self::write(&mut stream, &json!({
                        "kind": "capabilities", "identity": self.identity,
                        "request_id": request.request_id, "request_sha256": sha256(bytes),
                        "approved_request_sha256": self.config.approved_request_sha256,
                        "sequence":sequence, "monotonic_ns":monotonic_ns, "clock":"observer-monotonic",
                        "capabilities":capabilities, "live_profile_qualified":false
                    }), deadline).await?;
                }
                Operation::AuthoritySnapshot => {
                    let authority = request.authority.ok_or(ObserverError::Request)?;
                    let result = self
                        .controller
                        .authority_snapshot(authority, deadline.into_std())
                        .await;
                    let (sequence, monotonic_ns) = self.event()?;
                    let result = match result {
                        Ok(value) => json!({"availability":"available","value":value}),
                        Err(failure) => {
                            json!({"availability":"unavailable","reason":failure.reason,"work":failure.work,"input_digest":null})
                        }
                    };
                    Self::write(&mut stream, &json!({
                        "kind":"authority_snapshot", "identity":self.identity,
                        "request_id":request.request_id, "request_sha256":sha256(bytes),
                        "approved_request_sha256":self.config.approved_request_sha256,
                        "sequence":sequence, "monotonic_ns":monotonic_ns, "clock":"observer-monotonic",
                        "result":result, "live_profile_qualified":false
                    }), deadline).await?;
                }
            }
            Ok(())
        }

        pub async fn run(mut self) -> Result<(), ObserverError> {
            for _ in 0..self.config.max_sessions {
                let (stream, _) = self.listener.accept().await?;
                let deadline = tokio::time::Instant::now()
                    + Duration::from_millis(self.config.session_timeout_ms);
                let _ =
                    tokio::time::timeout_at(deadline, self.session(stream, deadline, Uuid::new_v4))
                        .await;
            }
            Err(ObserverError::ResourceLimit)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        struct Fixture {
            observer: Observer,
            directory: PathBuf,
        }

        impl Drop for Fixture {
            fn drop(&mut self) { let _ = fs::remove_dir_all(&self.directory); }
        }

        fn fixture() -> Fixture {
            let directory = std::env::temp_dir().join(format!("nonce-{}", Uuid::new_v4()));
            fs::create_dir(&directory).unwrap();
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
            let path = directory.join("observer.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let metadata = fs::symlink_metadata(&path).unwrap();
            let pid = std::process::id();
            let start = process_start_ticks(pid).unwrap();
            let config = SoakObserverConfig {
                directory: directory.clone(),
                source_revision: "a".repeat(40),
                approved_request_sha256: "b".repeat(64),
                peer_pid: pid,
                peer_start_ticks: start,
                session_timeout_ms: 500,
                max_sessions: 128,
            };
            validate_config(&config).unwrap();
            Fixture {
                directory,
                observer: Observer {
                    controller: casper::rust::soak_observer::ObserverController::new(
                        Uuid::nil().to_string(),
                    ),
                    listener,
                    _socket: SocketGuard {
                        path,
                        device: metadata.dev(),
                        inode: metadata.ino(),
                    },
                    config,
                    identity: Identity {
                        schema_version: 1,
                        incarnation: Uuid::nil().to_string(),
                        pid,
                        process_start_ticks: start,
                        declared_source_revision: "a".repeat(40),
                        executable_sha256: "c".repeat(64),
                        configuration_sha256: "d".repeat(64),
                        configuration_scope: "batch-a-public-config-v1",
                    },
                    owner_uid: fs::metadata("/proc/self").unwrap().uid(),
                    started: Instant::now(),
                    sequence: 0,
                },
            }
        }

        async fn receive(stream: &mut UnixStream) -> std::io::Result<serde_json::Value> {
            let size = stream.read_u32().await? as usize;
            assert!((1..=MAX_FRAME_BYTES).contains(&size));
            let mut bytes = vec![0; size];
            stream.read_exact(&mut bytes).await?;
            serde_json::from_slice(&bytes).map_err(std::io::Error::other)
        }

        #[tokio::test]
        async fn repeated_entropy_cannot_replay_a_request() {
            let mut fixture = fixture();
            let observer = &mut fixture.observer;
            let mut previous = None;
            for attempt in 0..2 {
                let mut client =
                    UnixStream::connect(observer.config.directory.join("observer.sock"))
                        .await
                        .unwrap();
                let (server, _) = observer.listener.accept().await.unwrap();
                let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
                let exchange = async {
                    let hello = receive(&mut client).await.unwrap();
                    let request = previous.get_or_insert_with(|| {
                        json!({
                            "schema_version": 1,
                            "request_id": Uuid::nil().to_string(),
                            "incarnation": hello["identity"]["incarnation"],
                            "challenge": hello["challenge"],
                            "executable_sha256": hello["identity"]["executable_sha256"],
                            "configuration_sha256": hello["identity"]["configuration_sha256"],
                            "approved_request_sha256": hello["approved_request_sha256"],
                            "operation": "capabilities"
                        })
                    });
                    let bytes = serde_json::to_vec(request).unwrap();
                    client.write_u32(bytes.len() as u32).await.unwrap();
                    client.write_all(&bytes).await.unwrap();
                    receive(&mut client).await
                };
                let (result, response) = tokio::time::timeout_at(deadline, async {
                    tokio::join!(observer.session(server, deadline, Uuid::nil), exchange)
                })
                .await
                .unwrap();
                if attempt == 0 {
                    assert!(result.is_ok());
                    assert_eq!(response.unwrap()["kind"], "capabilities");
                } else {
                    assert!(matches!(result, Err(ObserverError::Request)));
                    assert!(response.is_err());
                }
            }
        }

        #[tokio::test]
        async fn peer_admission_rejects_each_identity_mismatch_before_hello() {
            for mismatch in 0..3 {
                let mut fixture = fixture();
                let observer = &mut fixture.observer;
                match mismatch {
                    0 => observer.owner_uid ^= 1,
                    1 => observer.config.peer_pid = 0,
                    _ => observer.config.peer_start_ticks ^= 1,
                }
                let mut client =
                    UnixStream::connect(observer.config.directory.join("observer.sock"))
                        .await
                        .unwrap();
                let (server, _) = observer.listener.accept().await.unwrap();
                let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
                assert!(matches!(
                    observer.session(server, deadline, Uuid::nil).await,
                    Err(ObserverError::PeerIdentity)
                ));
                assert!(receive(&mut client).await.is_err());
                assert_eq!(observer.sequence, 0);
            }
        }

        #[tokio::test]
        async fn expired_write_emits_no_frame() {
            let (mut server, mut client) = UnixStream::pair().unwrap();
            let result = Observer::write(
                &mut server,
                &json!({"kind": "hello"}),
                tokio::time::Instant::now(),
            )
            .await;
            assert!(matches!(result, Err(ObserverError::ResourceLimit)));
            drop(server);
            let mut bytes = Vec::new();
            client.read_to_end(&mut bytes).await.unwrap();
            assert!(bytes.is_empty());
        }

        #[tokio::test]
        async fn directory_admission_matches_all_leaf_permission_bits() {
            let fixture = fixture();
            let uid = fs::metadata("/proc/self").unwrap().uid();
            for mode in 0..=0o7777 {
                fs::set_permissions(&fixture.directory, fs::Permissions::from_mode(mode)).unwrap();
                assert_eq!(
                    safe_directory(&fixture.directory, uid).is_ok(),
                    mode == 0o700,
                    "mode {mode:o}"
                );
            }
            fs::set_permissions(&fixture.directory, fs::Permissions::from_mode(0o700)).unwrap();
        }

        #[tokio::test]
        async fn cleanup_requires_both_socket_identity_fields() {
            let fixture = fixture();
            let original = &fixture.observer._socket;
            for (device, inode) in [
                (original.device ^ 1, original.inode),
                (original.device, original.inode ^ 1),
            ] {
                drop(SocketGuard {
                    path: original.path.clone(),
                    device,
                    inode,
                });
                assert!(original.path.exists());
            }
        }

        #[tokio::test]
        async fn exhausted_sequence_refuses_a_handshake() {
            let mut fixture = fixture();
            let observer = &mut fixture.observer;
            observer.sequence = u64::MAX;
            let mut client = UnixStream::connect(observer.config.directory.join("observer.sock"))
                .await
                .unwrap();
            let (server, _) = observer.listener.accept().await.unwrap();
            let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
            let result = observer.session(server, deadline, Uuid::nil).await;
            assert!(matches!(result, Err(ObserverError::ResourceLimit)));
            assert!(receive(&mut client).await.is_err());
            assert_eq!(observer.sequence, u64::MAX);
        }
    }
}
