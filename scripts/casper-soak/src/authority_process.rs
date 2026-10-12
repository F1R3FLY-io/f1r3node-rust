use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use casper_soak::{array, encoded, exclusive, file_hash, hash, number, parse, text, MAX_BYTES};
use eyre::{ensure, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub fn now() -> Result<u64> {
    ensure!(cfg!(target_os = "linux"), "Process control requires Linux.");
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    ensure!(
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) } == 0,
        "The host clock is unavailable."
    );
    Ok(ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64)
}

pub fn identity(pid: u32) -> Result<Value> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .ok_or_else(|| eyre::eyre!("The process record is invalid."))?
        .1
        .split_whitespace()
        .collect();
    ensure!(fields.len() > 19, "The process record is incomplete.");
    let mut executable = fs::File::open(format!("/proc/{pid}/exe"))?.take(1024 * MAX_BYTES + 1);
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        let count = executable.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        ensure!(
            total <= 1024 * MAX_BYTES,
            "The process executable exceeds its bound."
        );
        digest.update(&buffer[..count]);
    }
    Ok(
        json!({"pid":pid,"process_start_ticks":fields[19].parse::<u64>()?,"state":fields[0],"executable_sha256":format!("{:x}",digest.finalize())}),
    )
}

fn matches(actual: &Value, expected: &Value) -> bool {
    ["pid", "process_start_ticks", "executable_sha256"]
        .iter()
        .all(|key| actual[key] == expected[key] && !actual[key].is_null())
}

pub struct Owned {
    child: Child,
    config: Value,
    identity: Value,
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Owned {
    pub fn launch(config: &Value) -> Result<Self> {
        let path = Path::new(text(&config["path"])?);
        ensure!(
            path.is_absolute() && file_hash(path)? == config["sha256"],
            "The child executable differs."
        );
        let args = array(&config["arguments"])?;
        ensure!(
            args.len() <= 128,
            "The child argument count exceeds its bound."
        );
        let args = args.iter().map(text).collect::<Result<Vec<_>>>()?;
        ensure!(
            args.iter()
                .all(|arg| arg.len() <= 4096 && !arg.contains('\0')),
            "A child argument is invalid."
        );
        let cwd = Path::new(text(&config["working_directory"])?);
        ensure!(
            cwd.is_absolute() && cwd.is_dir(),
            "The child working directory is invalid."
        );
        let mut command = Command::new(path);
        let owner = identity(std::process::id())?;
        let args: Vec<_> = args
            .iter()
            .map(|arg| {
                arg.replace("{owner_pid}", &std::process::id().to_string())
                    .replace(
                        "{owner_start_ticks}",
                        &owner["process_start_ticks"].to_string(),
                    )
            })
            .collect();
        command
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        #[cfg(target_os = "linux")]
        unsafe {
            let parent = libc::getpid();
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    return Err(std::io::Error::other("The process owner exited."));
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let mut owned = Self {
            child,
            config: config.clone(),
            identity: Value::Null,
        };
        owned.identity = identity(owned.child.id())?;
        ensure!(
            owned.identity["executable_sha256"] == config["sha256"]
                && owned.child.try_wait()?.is_none(),
            "The child executable identity differs."
        );
        Ok(owned)
    }

    pub fn apply(&mut self, request: &Value) -> Result<Value> {
        ensure!(
            request["schema_version"] == 1,
            "The process schema differs."
        );
        let action = text(&request["action"])?;
        ensure!(
            ["pause", "restart"].contains(&action),
            "The process action is unsupported."
        );
        let deadline = number(&request["deadline_monotonic_ns"])?;
        let current = now()?;
        ensure!(
            deadline > current && deadline - current <= 300_000_000_000,
            "The process deadline is invalid."
        );
        let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
        ensure!(
            request["clock_id"] == format!("linux-monotonic:{}", boot.trim()),
            "The process clock differs."
        );
        let before = identity(self.child.id())?;
        ensure!(
            self.child.try_wait()?.is_none()
                && matches(&before, &self.identity)
                && matches(&before, &request["process"]),
            "The owned child identity differs."
        );
        let mut result = json!({"before":before,"status":"applied","action":action});
        if action == "pause" {
            let hold_ms = number(&request["hold_ms"])?;
            ensure!(
                (1..=30_000).contains(&hold_ms)
                    && now()?.saturating_add(hold_ms * 1_000_000 + 50_000_000) < deadline,
                "The pause interval exceeds its bound."
            );
            ensure!(
                unsafe { libc::kill(self.child.id() as i32, libc::SIGSTOP) } == 0,
                "The child pause failed."
            );
            let attempt = (|| -> Result<()> {
                loop {
                    ensure!(
                        now()? < deadline && self.child.try_wait()?.is_none(),
                        "The pause deadline expired or the child exited."
                    );
                    let state = identity(self.child.id())?;
                    ensure!(matches(&state, &before), "The paused child changed.");
                    if state["state"] == "T" || state["state"] == "t" {
                        result["stopped"] = state;
                        result["observed_state"] = json!("stopped");
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                std::thread::sleep(Duration::from_millis(hold_ms));
                Ok(())
            })();
            let resumed = unsafe { libc::kill(self.child.id() as i32, libc::SIGCONT) } == 0;
            attempt?;
            ensure!(resumed, "The child resume failed.");
            loop {
                ensure!(
                    now()? < deadline && self.child.try_wait()?.is_none(),
                    "The child did not resume before the deadline."
                );
                let state = identity(self.child.id())?;
                ensure!(matches(&state, &before), "The resumed child changed.");
                if state["state"] != "T" && state["state"] != "t" {
                    result["after"] = state;
                    result["resumed"] = json!(true);
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        } else {
            let socket = if self.config["observer_socket"].is_string() {
                use std::os::unix::fs::{FileTypeExt, MetadataExt};
                let path = Path::new(text(&self.config["observer_socket"])?);
                ensure!(
                    path.is_absolute(),
                    "The observer socket path must be absolute."
                );
                let metadata = fs::symlink_metadata(path)?;
                ensure!(
                    metadata.file_type().is_socket()
                        && metadata.uid() == unsafe { libc::geteuid() },
                    "The observer socket ownership differs."
                );
                let stream = UnixStream::connect(path)?;
                ensure!(
                    peer_pid(&stream)? == self.child.id(),
                    "The observer socket does not belong to the child."
                );
                Some((path.to_path_buf(), metadata.dev(), metadata.ino()))
            } else {
                None
            };
            self.child.kill()?;
            let status = self.child.wait()?;
            result["prior_exit"] = json!(true);
            result["prior_exit_code"] = json!(status.code());
            result["prior_exit_signal"] = json!(status.signal());
            if let Some((path, device, inode)) = socket {
                use std::os::unix::fs::{FileTypeExt, MetadataExt};
                match fs::symlink_metadata(&path) {
                    Ok(metadata) => {
                        ensure!(
                            metadata.file_type().is_socket()
                                && metadata.dev() == device
                                && metadata.ino() == inode,
                            "The predecessor socket changed before cleanup."
                        );
                        fs::remove_file(path)?;
                        result["socket_cleanup"] =
                            json!({"device":device,"inode":inode,"removed":true});
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
            ensure!(now()? < deadline, "The restart deadline expired.");
            let replacement = Self::launch(&self.config)?;
            let old = std::mem::replace(self, replacement);
            drop(old);
            result["after"] = self.identity.clone();
        }
        ensure!(now()? <= deadline, "The process receipt is late.");
        result["monotonic_ns"] = json!(now()?);
        Ok(result)
    }
}

fn receive(stream: &mut UnixStream) -> Result<Vec<u8>> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header)?;
    let size = u32::from_be_bytes(header) as u64;
    ensure!(
        size > 0 && size <= MAX_BYTES,
        "The process frame size is invalid."
    );
    let mut bytes = vec![0; size as usize];
    stream.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn send(stream: &mut UnixStream, value: &Value) -> Result<()> {
    let bytes = encoded(value)?;
    ensure!(
        bytes.len() as u64 <= MAX_BYTES,
        "The process response exceeds its bound."
    );
    stream.write_all(&(bytes.len() as u32).to_be_bytes())?;
    stream.write_all(&bytes)?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn peer_pid(stream: &UnixStream) -> Result<u32> {
    use std::os::fd::AsRawFd;
    let mut peer = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut size = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut peer as *mut _ as *mut _,
                &mut size,
            )
        } == 0
            && peer.uid == unsafe { libc::geteuid() }
            && peer.pid > 0,
        "The process peer differs."
    );
    Ok(peer.pid as u32)
}

#[cfg(not(target_os = "linux"))]
fn peer_pid(_: &UnixStream) -> Result<u32> { eyre::bail!("Process control requires Linux.") }

pub fn call(owner: &Value, request: &Value, timeout: Duration) -> Result<Value> {
    let mut stream = UnixStream::connect(text(&owner["socket"])?)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let pid = peer_pid(&stream)?;
    ensure!(
        matches(&identity(pid)?, &owner["identity"]),
        "The process owner identity differs."
    );
    send(&mut stream, request)?;
    let response = parse(&receive(&mut stream)?)?;
    ensure!(
        response["request_sha256"] == hash(&encoded(request)?) && response["status"] == "applied",
        "The process owner rejected the request."
    );
    ensure!(
        matches(&identity(pid)?, &owner["identity"]),
        "The process owner changed during the request."
    );
    Ok(response)
}

pub fn serve(
    config: &Value,
    root: &Path,
    lifetime: Duration,
    capture: impl Fn(&Value, &Path) -> Result<Value>,
) -> Result<()> {
    now()?;
    ensure!(
        lifetime <= Duration::from_secs(300) && !lifetime.is_zero(),
        "The owner lifetime is invalid."
    );
    ensure!(
        root.is_absolute() && !root.exists(),
        "The process output must be new and absolute."
    );
    fs::create_dir(root)?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    let socket = root.join("control.sock");
    let listener = UnixListener::bind(&socket)?;
    listener.set_nonblocking(true)?;
    let mut child = Owned::launch(config)?;
    let end = Instant::now() + lifetime;
    let owner = json!({"socket":socket,"identity":identity(std::process::id())?,"child":child.identity,
        "expires_monotonic_ns":now()?.saturating_add(lifetime.as_nanos() as u64)});
    exclusive(&root.join("owner.json"), &encoded(&owner)?, true)?;
    let mut seen = std::collections::BTreeSet::new();
    while Instant::now() < end && child.child.try_wait()?.is_none() {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let remaining = end
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(1));
                if remaining.is_zero() {
                    break;
                }
                stream.set_read_timeout(Some(remaining))?;
                stream.set_write_timeout(Some(remaining))?;
                let attempt = (|| -> Result<Value> {
                    peer_pid(&stream)?;
                    let bytes = receive(&mut stream)?;
                    let request = parse(&bytes)?;
                    ensure!(
                        request["schema_version"] == 1,
                        "The process request schema differs."
                    );
                    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
                    ensure!(
                        request["clock_id"] == format!("linux-monotonic:{}", boot.trim()),
                        "The process request clock differs."
                    );
                    let digest = hash(&encoded(&request)?);
                    ensure!(
                        seen.len() < 1024 && seen.insert(digest.clone()),
                        "The process request repeats or exceeds its bound."
                    );
                    let remaining = end.saturating_duration_since(Instant::now()).as_nanos() as u64;
                    ensure!(
                        number(&request["deadline_monotonic_ns"])?
                            <= now()?.saturating_add(remaining),
                        "The request exceeds the owner lifetime."
                    );
                    let mut response = if request["action"] == "capture" {
                        let binding = &request["binding"];
                        ensure!(
                            child.child.try_wait()?.is_none()
                                && binding["node_pid"] == child.child.id()
                                && binding["process_start_ticks"]
                                    == child.identity["process_start_ticks"]
                                && binding["executable_sha256"]
                                    == child.identity["executable_sha256"],
                            "The capture child identity differs."
                        );
                        ensure!(
                            binding["socket"] == config["observer_socket"]
                                && config["observer_socket"].is_string(),
                            "The capture socket differs from the owner configuration."
                        );
                        let remaining = number(&request["deadline_monotonic_ns"])?
                            .checked_sub(now()?)
                            .ok_or_else(|| eyre::eyre!("The capture deadline expired."))?;
                        ensure!(
                            number(&binding["timeout_ms"])? <= remaining / 1_000_000,
                            "The capture timeout exceeds its deadline."
                        );
                        let directory = root.join(format!("capture-{:04}", seen.len()));
                        let report = capture(&request, &directory)?;
                        json!({"status":"applied","capture_root":directory,"report_sha256":hash(&encoded(&report)?)})
                    } else {
                        child.apply(&request)?
                    };
                    response["request_sha256"] = digest.into();
                    exclusive(
                        &root.join(format!("receipt-{:04}.json", seen.len())),
                        &encoded(&response)?,
                        true,
                    )?;
                    Ok(response)
                })();
                let response = attempt
                    .unwrap_or_else(|error| json!({"status":"rejected","error":error.to_string()}));
                let _ = send(&mut stream, &response);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5))
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
