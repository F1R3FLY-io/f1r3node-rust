use std::path::PathBuf;
use clap::{Parser, Subcommand};
use eyre::Result;
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    action: Action,
}
#[derive(Subcommand)]
enum Action {
    Probe,
    StopOwned {
        owner: String,
    },
    PreferOom {
        owner: String,
    },
    VerifyDomain {
        record: PathBuf,
    },
    Handled {
        marker: PathBuf,
    },
    Watch {
        pid: i32,
        identity: String,
        ready: PathBuf,
    },
}
pub fn main() -> i32 {
    let args = Args::parse_from(std::env::args_os().skip(1));
    match execute(args.action) {
        Ok(()) => 0,
        Err(_) => {
            eprintln!("The Linux host-control operation could not be verified.");
            2
        }
    }
}
#[cfg(not(target_os = "linux"))]
fn execute(_: Action) -> Result<()> { eyre::bail!("Linux pidfd support is required.") }
#[cfg(target_os = "linux")]
fn execute(action: Action) -> Result<()> { linux::execute(action) }
#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::CString;
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::path::Path;
    use std::time::{Duration, Instant};
    use eyre::{ensure, eyre};
    use super::*;
    const RECORD_LIMIT: u64 = 65536;
    fn open_pid(pid: i32) -> io::Result<File> {
        if pid <= 0 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd as i32) })
    }
    fn signal(fd: &File, signal: i32) -> io::Result<()> {
        let status = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                fd.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if status < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
    fn exited(fd: &File, timeout: i32) -> io::Result<bool> {
        let mut event = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let status = unsafe { libc::poll(&mut event, 1, timeout) };
        if status < 0 {
            return Err(io::Error::last_os_error());
        }
        if status == 0 {
            return Ok(false);
        }
        if event.revents & (libc::POLLERR | libc::POLLNVAL) != 0
            || event.revents & libc::POLLIN == 0
        {
            return Err(io::Error::other("The process exit is unconfirmed."));
        }
        Ok(true)
    }
    fn child(parent: &File, name: &str, directory: bool, write: bool) -> io::Result<File> {
        let name = CString::new(name)?;
        let flags = libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | libc::O_NONBLOCK
            | if directory { libc::O_DIRECTORY } else { 0 }
            | if write {
                libc::O_WRONLY
            } else {
                libc::O_RDONLY
            };
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    fn directory(path: &Path) -> io::Result<File> {
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
    }
    fn bounded(file: File, limit: u64) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            return Err(io::Error::other("The input exceeds its bound."));
        }
        Ok(bytes)
    }
    fn trusted(file: &File, directory: bool) -> Result<()> {
        let metadata = file.metadata()?;
        ensure!(
            metadata.uid() == 0
                && metadata.mode() & 0o022 == 0
                && if directory {
                    metadata.is_dir()
                } else {
                    metadata.is_file()
                },
            "The record entry is not trusted."
        );
        Ok(())
    }
    fn domain_record(path: &Path) -> Result<Vec<u8>> {
        let path = path
            .to_str()
            .ok_or_else(|| eyre!("The record path is invalid."))?;
        ensure!(
            path.starts_with('/') && !path.contains("/.") && !path.ends_with('/'),
            "The record path is invalid."
        );
        let parts: Vec<_> = path[1..].split('/').collect();
        ensure!(
            parts.iter().all(|part| !part.is_empty()),
            "The record path is invalid."
        );
        let mut fd = directory(Path::new("/"))?;
        trusted(&fd, true)?;
        for (index, part) in parts.iter().enumerate() {
            let is_directory = index + 1 != parts.len();
            fd = child(&fd, part, is_directory, false)?;
            trusted(&fd, is_directory)?;
        }
        Ok(bounded(fd, RECORD_LIMIT)?)
    }
    fn check_domain(bytes: &[u8], uid: u32, cgroup: &str) -> Result<()> {
        let record = casper_soak::parse(bytes)?;
        ensure!(
            record["uid"].as_u64() == Some(u64::from(uid)),
            "The record UID differs."
        );
        let unit = casper_soak::text(&record["unit"])?;
        let expected = casper_soak::text(&record["cgroup"])?;
        ensure!(
            !unit.is_empty() && !unit.contains('/') && expected.starts_with('/'),
            "The domain record is invalid."
        );
        ensure!(
            cgroup.lines().collect::<Vec<_>>() == vec![format!("0::{expected}")],
            "The process is outside the recorded domain."
        );
        Ok(())
    }
    struct Owned {
        process: File,
        directory: File,
    }
    fn owned(owner: &str) -> Result<(Vec<Owned>, bool)> {
        ensure!(
            !owner.is_empty() && owner.len() <= 256 && !owner.chars().any(char::is_control),
            "The owner marker is invalid."
        );
        let marker = format!("SOAK_PROCESS_OWNER={owner}");
        let entries = fs::read_dir("/proc")?.collect::<io::Result<Vec<_>>>()?;
        let mut pids: Vec<i32> = entries
            .iter()
            .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
            .collect();
        pids.sort_unstable();
        let mut result = Vec::new();
        let mut failed = false;
        for pid in pids {
            let process = match open_pid(pid) {
                Ok(fd) => fd,
                Err(error) if error.raw_os_error() == Some(libc::ESRCH) => continue,
                Err(_) => {
                    failed = true;
                    continue;
                }
            };
            let scan = (|| -> Result<Option<Owned>> {
                let directory = directory(Path::new(&format!("/proc/{pid}")))?;
                let status = String::from_utf8(bounded(
                    child(&directory, "status", false, false)?,
                    RECORD_LIMIT,
                )?)?;
                let uid_line = status
                    .lines()
                    .find(|line| line.starts_with("Uid:"))
                    .ok_or_else(|| eyre!("The process UID is unavailable."))?;
                let uids = uid_line[4..]
                    .split_whitespace()
                    .map(str::parse::<u32>)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                ensure!(uids.len() == 4, "The process UID is invalid.");
                if uids[1] != unsafe { libc::geteuid() } {
                    return Ok(None);
                }
                let environment = bounded(
                    child(&directory, "environ", false, false)?,
                    16 * 1024 * 1024,
                )?;
                if !environment
                    .split(|b| *b == 0)
                    .any(|value| value == marker.as_bytes())
                {
                    return Ok(None);
                }
                if exited(&process, 0)? {
                    failed = true;
                    return Ok(None);
                }
                Ok(Some(Owned {
                    process: process.try_clone()?,
                    directory,
                }))
            })();
            match scan {
                Ok(Some(value)) => result.push(value),
                Ok(None) => (),
                Err(_) => {
                    if !exited(&process, 0)? {
                        failed = true;
                    }
                }
            }
        }
        Ok((result, failed))
    }
    fn stop(owner: &str) -> Result<()> {
        let (processes, mut failed) = owned(owner)?;
        for owned in &processes {
            if let Err(error) = signal(&owned.process, libc::SIGKILL) {
                if error.raw_os_error() != Some(libc::ESRCH) {
                    failed = true;
                }
            }
        }
        let deadline = Instant::now() + Duration::from_millis(750);
        for owned in &processes {
            loop {
                let remaining = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis() as i32;
                match exited(&owned.process, remaining) {
                    Ok(true) => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted && remaining > 0 => {
                        continue
                    }
                    _ => {
                        failed = true;
                        break;
                    }
                }
            }
        }
        ensure!(!failed, "Writer termination is unconfirmed.");
        Ok(())
    }
    fn prefer_oom(owner: &str) -> Result<()> {
        let (processes, mut failed) = owned(owner)?;
        for owned in processes {
            if child(&owned.directory, "oom_score_adj", false, true)
                .and_then(|mut file| file.write_all(b"1000\n"))
                .is_err()
                && !exited(&owned.process, 0)?
            {
                failed = true;
            }
        }
        ensure!(!failed, "The OOM preference is unconfirmed.");
        Ok(())
    }
    fn watch(pid: i32, identity: &str, ready: &Path) -> Result<()> {
        let process = open_pid(pid)?;
        let directory = directory(Path::new(&format!("/proc/{pid}")))?;
        let stat = String::from_utf8(bounded(
            child(&directory, "stat", false, false)?,
            RECORD_LIMIT,
        )?)?;
        let fields: Vec<_> = stat
            .rsplit_once(") ")
            .ok_or_else(|| eyre!("The process identity is invalid."))?
            .1
            .split_whitespace()
            .collect();
        ensure!(
            fields.get(19) == Some(&identity) && !exited(&process, 0)?,
            "The driver identity differs."
        );
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(ready)?
            .write_all(format!("{}\n", std::process::id()).as_bytes())?;
        loop {
            match exited(&process, -1) {
                Ok(true) => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                _ => return Err(eyre!("The driver exit is unconfirmed.")),
            }
        }
    }
    pub(super) fn execute(action: Action) -> Result<()> {
        match action {
            Action::Probe => {
                let fd = open_pid(std::process::id() as i32)?;
                signal(&fd, 0)?;
            }
            Action::Handled { marker } => ensure!(
                casper_soak::regular(&marker, 8)? == b"handled\n",
                "The exit marker is invalid."
            ),
            Action::StopOwned { owner } => stop(&owner)?,
            Action::PreferOom { owner } => prefer_oom(&owner)?,
            Action::VerifyDomain { record } => check_domain(
                &domain_record(&record)?,
                unsafe { libc::getuid() },
                &fs::read_to_string("/proc/self/cgroup")?,
            )?,
            Action::Watch {
                pid,
                identity,
                ready,
            } => watch(pid, &identity, &ready)?,
        }
        Ok(())
    }
}
