use std::fs;
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "linux", test))]
use casper_soak::MAX_BYTES;
use casper_soak::{encoded, exclusive, hash, manifest, number, parse, text};
use eyre::{ensure, Result};
use serde_json::{json, Value};

#[path = "authority_mapping.rs"]
pub(super) mod mapping;

#[derive(Clone, Debug)]
pub struct Binding {
    pub socket: PathBuf,
    pub node_pid: u32,
    pub process_start_ticks: u64,
    pub source_revision: String,
    pub executable_sha256: String,
    pub configuration_sha256: String,
    pub approved_request_sha256: String,
    pub request_id: String,
    pub timeout_ms: u64,
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
            }
        })
}

impl Binding {
    pub fn from_value(value: &Value) -> Result<Self> {
        ensure!(
            casper_soak::object(value)?.len() == 9,
            "The binding field inventory differs."
        );
        let binding = Self {
            socket: text(&value["socket"])?.into(),
            node_pid: number(&value["node_pid"])?.try_into()?,
            process_start_ticks: number(&value["process_start_ticks"])?,
            source_revision: text(&value["source_revision"])?.into(),
            executable_sha256: text(&value["executable_sha256"])?.into(),
            configuration_sha256: text(&value["configuration_sha256"])?.into(),
            approved_request_sha256: text(&value["approved_request_sha256"])?.into(),
            request_id: text(&value["request_id"])?.into(),
            timeout_ms: number(&value["timeout_ms"])?,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.socket.is_absolute()
                && self.socket.components().all(|c| matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                ))
                && self.node_pid > 0
                && self.node_pid <= i32::MAX as u32
                && self.process_start_ticks > 0
                && (50..=30_000).contains(&self.timeout_ms)
                && uuid(&self.request_id),
            "The observer binding is invalid."
        );
        manifest::hex(&json!(self.source_revision), 40)?;
        for digest in [
            &self.executable_sha256,
            &self.configuration_sha256,
            &self.approved_request_sha256,
        ] {
            manifest::hex(&json!(digest), 64)?;
        }
        Ok(())
    }
}

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn request(binding: &Binding, hello: &Value, authority: &Value) -> Result<Value> {
    binding.validate()?;
    ensure!(
        authority.is_object(),
        "The authority request must be an object."
    );
    let identity = &hello["identity"];
    let sequence = number(&hello["sequence"])?;
    let challenge = text(&hello["challenge"])?;
    let (nonce, suffix) = challenge
        .rsplit_once(':')
        .ok_or_else(|| eyre::eyre!("The observer challenge is invalid."))?;
    ensure!(
        hello["kind"] == "hello"
            && hello["permission"] == "read-only"
            && hello["clock"] == "observer-monotonic"
            && hello["max_frame_bytes"] == MAX_BYTES
            && hello["approved_request_sha256"] == binding.approved_request_sha256
            && sequence > 0
            && uuid(nonce)
            && suffix == sequence.to_string(),
        "The observer greeting differs from the protocol."
    );
    number(&hello["monotonic_ns"])?;
    ensure!(
        identity["schema_version"] == 1
            && uuid(text(&identity["incarnation"])?)
            && identity["pid"] == binding.node_pid
            && identity["process_start_ticks"] == binding.process_start_ticks
            && identity["declared_source_revision"] == binding.source_revision
            && identity["executable_sha256"] == binding.executable_sha256
            && identity["configuration_sha256"] == binding.configuration_sha256
            && identity["configuration_scope"] == "batch-a-public-config-v1",
        "The observer identity differs from the binding."
    );
    Ok(json!({
        "schema_version":1,
        "request_id":binding.request_id,
        "incarnation":identity["incarnation"],
        "challenge":challenge,
        "executable_sha256":binding.executable_sha256,
        "configuration_sha256":binding.configuration_sha256,
        "approved_request_sha256":binding.approved_request_sha256,
        "operation":"authority_snapshot",
        "authority":authority
    }))
}

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub fn validate_response(hello: &Value, request_bytes: &[u8], response: &Value) -> Result<()> {
    let request = parse(request_bytes)?;
    ensure!(
        response["kind"] == "authority_snapshot"
            && response["identity"] == hello["identity"]
            && response["request_id"] == request["request_id"]
            && response["request_sha256"] == hash(request_bytes)
            && response["approved_request_sha256"] == request["approved_request_sha256"]
            && response["clock"] == "observer-monotonic"
            && response["live_profile_qualified"] == false
            && number(&response["sequence"])? > number(&hello["sequence"])?
            && number(&response["monotonic_ns"])? >= number(&hello["monotonic_ns"])?,
        "The observer response is not bound to this request."
    );
    let result = &response["result"];
    match text(&result["availability"])? {
        "available" => {
            let value = &result["value"];
            ensure!(
                value["scope"] == "batch-b2-detached-authority-evaluation"
                    && value["live_profile_qualified"] == false
                    && value["request"] == request["authority"],
                "The authority result does not describe the requested evaluation."
            );
            manifest::hex(&value["snapshot_digest"], 64)?;
            manifest::hex(&value["authority_digest"], 64)?;
        }
        "unavailable" => {
            ensure!(
                !text(&result["reason"])?.is_empty() && result["input_digest"].is_null(),
                "The unavailable result is malformed."
            );
        }
        _ => eyre::bail!("The authority result availability is unsupported."),
    }
    Ok(())
}

fn retain(output: &Path, name: &str, bytes: &[u8], records: &mut Vec<Value>) -> Result<()> {
    exclusive(&output.join(name), bytes, true)?;
    records.push(json!({"path":name,"sha256":hash(bytes),"bytes":bytes.len()}));
    Ok(())
}

pub fn collect(binding_bytes: &[u8], authority_bytes: &[u8], output: &Path) -> Result<Value> {
    let binding = Binding::from_value(&parse(binding_bytes)?)?;
    binding.validate()?;
    let authority = parse(authority_bytes)?;
    ensure!(
        authority.is_object(),
        "The authority request must be an object."
    );
    ensure!(
        cfg!(target_os = "linux"),
        "The observer client requires Linux."
    );
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(output)?;
    let mut records = Vec::new();
    retain(output, "binding.json", binding_bytes, &mut records)?;
    retain(output, "authority.json", authority_bytes, &mut records)?;
    #[cfg(target_os = "linux")]
    let attempt = linux::exchange(&binding, &authority, output, &mut records);
    #[cfg(not(target_os = "linux"))]
    let attempt: Result<Value> = Err(eyre::eyre!("The observer client requires Linux."));
    let mut report = match attempt {
        Ok(response) => {
            let mapped = match mapping::map(&response) {
                Ok(value) => value,
                Err(error) => {
                    json!({"schema_version":1,"status":"rejected","error":error.to_string(),
                    "qualification":"pending","profile_verdict":"blocked","soak_verdict":"non_passing"})
                }
            };
            retain(output, "mapping.json", &encoded(&mapped)?, &mut records)?;
            json!({
            "schema_version":1,"status":"captured",
            "availability":response["result"]["availability"],
            "mapping_status":mapped["status"],
            "qualification":"pending","profile_verdict":"pending","soak_verdict":"non_passing",
            "node_launch_count":0,"artifacts":records
            })
        }
        Err(error) => json!({
            "schema_version":1,"status":"rejected","error":error.to_string(),
            "qualification":"pending","profile_verdict":"pending","soak_verdict":"non_passing",
            "node_launch_count":0,"artifacts":records
        }),
    };
    report["client"] = json!({
        "executable_sha256":casper_soak::file_hash(&std::env::current_exe()?)?,
        "version":env!("CARGO_PKG_VERSION"),
        "source_digests":{
            "scripts/casper-soak/src/authority_observer.rs":hash(include_bytes!("authority_observer.rs")),
            "scripts/casper-soak/src/authority_mapping.rs":hash(include_bytes!("authority_mapping.rs")),
            "scripts/casper-soak/src/bin/casper-authority-observe.rs":hash(include_bytes!("bin/casper-authority-observe.rs")),
            "scripts/casper-soak/src/lib.rs":hash(include_bytes!("lib.rs")),
            "scripts/casper-soak/src/manifest.rs":hash(include_bytes!("manifest.rs"))
        }
    });
    exclusive(&output.join("report.json"), &encoded(&report)?, true)?;
    Ok(report)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::File;
    use std::io::{ErrorKind, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    use sha2::{Digest, Sha256};

    use super::*;

    fn remaining(deadline: Instant) -> Result<Duration> {
        let value = deadline.saturating_duration_since(Instant::now());
        ensure!(!value.is_zero(), "The observer deadline expired.");
        Ok(value)
    }

    fn wait(stream: &UnixStream, event: i16, deadline: Instant) -> Result<()> {
        loop {
            let timeout = remaining(deadline)?.as_millis().clamp(1, i32::MAX as u128) as i32;
            let mut poll = libc::pollfd {
                fd: stream.as_raw_fd(),
                events: event,
                revents: 0,
            };
            let result = unsafe { libc::poll(&mut poll, 1, timeout) };
            if result < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == ErrorKind::Interrupted {
                    continue;
                }
                return Err(error.into());
            }
            if result > 0 {
                return Ok(());
            }
        }
    }

    fn connect(path: &Path, deadline: Instant) -> Result<UnixStream> {
        let bytes = path.as_os_str().as_bytes();
        let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        ensure!(
            bytes.len() < address.sun_path.len() && !bytes.contains(&0),
            "The socket path is invalid."
        );
        address.sun_family = libc::AF_UNIX as libc::sa_family_t;
        for (target, byte) in address.sun_path.iter_mut().zip(bytes) {
            *target = *byte as libc::c_char;
        }
        let fd = unsafe {
            libc::socket(
                libc::AF_UNIX,
                libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0,
            )
        };
        ensure!(fd >= 0, "The observer socket could not be created.");
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let stream = UnixStream::from(owned);
        let result = unsafe {
            libc::connect(
                stream.as_raw_fd(),
                &address as *const _ as *const libc::sockaddr,
                std::mem::size_of_val(&address) as libc::socklen_t,
            )
        };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            ensure!(
                error.raw_os_error() == Some(libc::EINPROGRESS),
                "The observer connection failed: {error}"
            );
            wait(&stream, libc::POLLOUT, deadline)?;
            if let Some(error) = stream.take_error()? {
                return Err(error.into());
            }
        }
        remaining(deadline)?;
        Ok(stream)
    }

    fn read_exact(stream: &mut UnixStream, bytes: &mut [u8], deadline: Instant) -> Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            remaining(deadline)?;
            match stream.read(&mut bytes[offset..]) {
                Ok(0) => eyre::bail!("The observer frame is truncated."),
                Ok(n) => offset += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    wait(stream, libc::POLLIN, deadline)?
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    fn frame(stream: &mut UnixStream, deadline: Instant) -> Result<Vec<u8>> {
        let mut prefix = [0; 4];
        read_exact(stream, &mut prefix, deadline)?;
        let size = u32::from_be_bytes(prefix) as usize;
        ensure!(
            (1..=MAX_BYTES as usize).contains(&size),
            "The observer frame exceeds its bounds."
        );
        let mut bytes = vec![0; size];
        read_exact(stream, &mut bytes, deadline)?;
        Ok(bytes)
    }

    fn send(stream: &mut UnixStream, bytes: &[u8], deadline: Instant) -> Result<()> {
        ensure!(
            !bytes.is_empty() && bytes.len() as u64 <= MAX_BYTES,
            "The observer request exceeds its bounds."
        );
        let prefix = (bytes.len() as u32).to_be_bytes();
        for mut part in [prefix.as_slice(), bytes] {
            while !part.is_empty() {
                remaining(deadline)?;
                match stream.write(part) {
                    Ok(0) => eyre::bail!("The observer request could not be sent."),
                    Ok(n) => part = &part[n..],
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(e) if e.kind() == ErrorKind::WouldBlock => {
                        wait(stream, libc::POLLOUT, deadline)?
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Ok(())
    }

    fn process_start(pid: u32) -> Result<u64> {
        let bytes = casper_soak::regular(Path::new(&format!("/proc/{pid}/stat")), 4096)?;
        let stat = std::str::from_utf8(&bytes)?;
        let (_, suffix) = stat
            .rsplit_once(')')
            .ok_or_else(|| eyre::eyre!("The process record is invalid."))?;
        Ok(suffix
            .split_whitespace()
            .nth(19)
            .ok_or_else(|| eyre::eyre!("The process start time is absent."))?
            .parse()?)
    }

    fn peer(stream: &UnixStream, binding: &Binding, deadline: Instant) -> Result<()> {
        let mut credentials: libc::ucred = unsafe { std::mem::zeroed() };
        let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut credentials as *mut _ as *mut libc::c_void,
                &mut size,
            )
        };
        ensure!(
            result == 0 && size as usize == std::mem::size_of_val(&credentials),
            "The socket peer could not be verified."
        );
        ensure!(
            credentials.pid == binding.node_pid as i32
                && credentials.uid == unsafe { libc::geteuid() },
            "The socket peer identity differs."
        );
        ensure!(
            process_start(binding.node_pid)? == binding.process_start_ticks,
            "The node process was replaced."
        );
        let mut executable = File::open(format!("/proc/{}/exe", binding.node_pid))?;
        ensure!(
            executable.metadata()?.is_file() && executable.metadata()?.len() <= 512 * MAX_BYTES,
            "The node executable exceeds its bounds."
        );
        let mut digest = Sha256::new();
        let mut buffer = [0; 65_536];
        let mut total = 0u64;
        loop {
            remaining(deadline)?;
            let n = executable.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            ensure!(
                total <= 512 * MAX_BYTES,
                "The node executable grew beyond its bound."
            );
            digest.update(&buffer[..n]);
        }
        ensure!(
            format!("{:x}", digest.finalize()) == binding.executable_sha256,
            "The actual node executable digest differs."
        );
        ensure!(
            process_start(binding.node_pid)? == binding.process_start_ticks,
            "The node process was replaced."
        );
        Ok(())
    }

    pub fn exchange(
        binding: &Binding,
        authority: &Value,
        output: &Path,
        records: &mut Vec<Value>,
    ) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_millis(binding.timeout_ms);
        let mut stream = connect(&binding.socket, deadline)?;
        peer(&stream, binding, deadline)?;
        let hello_bytes = frame(&mut stream, deadline)?;
        retain(output, "hello.json", &hello_bytes, records)?;
        let hello = parse(&hello_bytes)?;
        let request = request(binding, &hello, authority)?;
        let request_bytes = serde_json::to_vec(&request)?;
        retain(output, "request.json", &request_bytes, records)?;
        send(&mut stream, &request_bytes, deadline)?;
        let response_bytes = frame(&mut stream, deadline)?;
        retain(output, "response.json", &response_bytes, records)?;
        let response = parse(&response_bytes)?;
        validate_response(&hello, &request_bytes, &response)?;
        peer(&stream, binding, deadline)?;
        remaining(deadline)?;
        Ok(response)
    }
}
