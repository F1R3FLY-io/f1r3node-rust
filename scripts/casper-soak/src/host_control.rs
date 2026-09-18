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

    #[cfg(test)]
    mod tests {
        use std::os::unix::fs::{symlink, PermissionsExt};
        use std::process::{Child, Command, Stdio};
        use std::thread;

        use super::*;

        struct Process(Child);
        impl Drop for Process {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        fn process(owner: &str) -> Process {
            assert!(
                Path::new("/.dockerenv").is_file(),
                "Run process fixtures in an isolated container."
            );
            let mut child = Process(
                Command::new("/bin/sh")
                    .args(["-c", "printf 'ready\\n'; read -r _"])
                    .env("SOAK_PROCESS_OWNER", owner)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let mut receipt = [0; 6];
            child
                .0
                .stdout
                .take()
                .unwrap()
                .read_exact(&mut receipt)
                .unwrap();
            assert_eq!(&receipt, b"ready\n");
            child
        }
        fn marker() -> String {
            format!(
                "fixture-{}-{:?}",
                std::process::id(),
                thread::current().id()
            )
        }
        fn identity(pid: u32) -> String {
            fs::read_to_string(format!("/proc/{pid}/stat"))
                .unwrap()
                .rsplit_once(") ")
                .unwrap()
                .1
                .split_whitespace()
                .nth(19)
                .unwrap()
                .to_owned()
        }

        #[test]
        fn pidfd_exit_cannot_signal_an_unrelated_process() {
            let mut original = process(&marker());
            let fd = open_pid(original.0.id() as i32).unwrap();
            original.0.kill().unwrap();
            original.0.wait().unwrap();
            assert!(exited(&fd, 0).unwrap());
            let mut unrelated = process("unrelated");
            assert_eq!(
                signal(&fd, libc::SIGKILL).unwrap_err().raw_os_error(),
                Some(libc::ESRCH)
            );
            assert!(unrelated.0.try_wait().unwrap().is_none());
            assert!(open_pid(0).is_err());
            assert!(open_pid(-1).is_err());
        }

        #[test]
        fn stop_kills_only_exact_owner_markers() {
            let owner = marker();
            let mut first = process(&owner);
            let mut second = process(&owner);
            let mut different = process(&format!("{owner}-suffix"));
            stop(&owner).unwrap();
            assert!(first.0.wait().unwrap().code().is_none());
            assert!(second.0.wait().unwrap().code().is_none());
            assert!(different.0.try_wait().unwrap().is_none());
            assert!(stop("").is_err());
            stop(&owner).unwrap();
        }

        #[test]
        fn oom_preference_uses_the_pinned_process_directory() {
            let owner = marker();
            let original = process(&owner);
            let different = process("unrelated");
            let path = format!("/proc/{}/oom_score_adj", different.0.id());
            let before = fs::read_to_string(&path).unwrap();
            prefer_oom(&owner).unwrap();
            assert_eq!(
                fs::read_to_string(format!("/proc/{}/oom_score_adj", original.0.id())).unwrap(),
                "1000\n"
            );
            assert_eq!(fs::read_to_string(path).unwrap(), before);
            let directory = directory(Path::new(&format!("/proc/{}", original.0.id()))).unwrap();
            drop(original);
            assert!(child(&directory, "oom_score_adj", false, true).is_err());
        }

        #[test]
        fn watcher_requires_identity_and_kernel_exit() {
            let mut original = process(&marker());
            let pid = original.0.id() as i32;
            let id = identity(original.0.id());
            let temp = tempfile::tempdir().unwrap();
            let ready = temp.path().join("ready");
            assert!(watch(pid, "wrong", &ready).is_err());
            assert!(!ready.exists());
            let output = ready.clone();
            let watcher = thread::spawn(move || watch(pid, &id, &output));
            let deadline = Instant::now() + Duration::from_secs(3);
            while !ready.exists() {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(10));
            }
            assert!(!watcher.is_finished());
            original.0.kill().unwrap();
            original.0.wait().unwrap();
            watcher.join().unwrap().unwrap();
        }

        #[test]
        fn handled_marker_requires_exact_bytes() {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("handled");
            fs::write(&path, b"handled\n").unwrap();
            execute(Action::Handled {
                marker: path.clone(),
            })
            .unwrap();
            for bytes in [
                b"handled\nextra".as_slice(),
                b"handled\n\0",
                b"handled",
                b"",
                b"unknown\n",
            ] {
                fs::write(&path, bytes).unwrap();
                assert!(execute(Action::Handled {
                    marker: path.clone()
                })
                .is_err());
            }
            let link = temp.path().join("link");
            symlink(&path, &link).unwrap();
            assert!(execute(Action::Handled { marker: link }).is_err());
        }

        #[test]
        fn record_schema_rejects_boolean_uid_duplicates_and_foreign_cgroups() {
            let good = br#"{"uid":0,"unit":"run.scope","cgroup":"/run"}"#;
            check_domain(good, 0, "0::/run\n").unwrap();
            assert!(check_domain(good, 1, "0::/run\n").is_err());
            assert!(check_domain(good, 0, "0::/foreign\n").is_err());
            assert!(check_domain(good, 0, "0::/run\n0::/run\n").is_err());
            for bytes in [
                br#"{"uid":false,"unit":"run.scope","cgroup":"/run"}"#.as_slice(),
                br#"{"uid":0,"uid":1,"unit":"run.scope","cgroup":"/run"}"#,
                br#"{"uid":0,"unit":"a/b","cgroup":"/run"}"#,
                br#"{"uid":0,"unit":"run.scope","cgroup":"run"}"#,
                br#"{"uid":0,"unit":"","cgroup":"/run"}"#,
            ] {
                assert!(check_domain(bytes, 0, "0::/run\n").is_err());
            }
        }

        #[test]
        fn descriptor_traversal_rejects_links_and_pins_renamed_ancestors() {
            let temp = tempfile::tempdir().unwrap();
            let original = temp.path().join("original");
            fs::create_dir(&original).unwrap();
            fs::write(original.join("record"), b"original").unwrap();
            let descriptor = directory(&original).unwrap();
            fs::rename(&original, temp.path().join("moved")).unwrap();
            fs::create_dir(&original).unwrap();
            fs::write(original.join("record"), b"substitute").unwrap();
            assert_eq!(
                bounded(child(&descriptor, "record", false, false).unwrap(), 64).unwrap(),
                b"original"
            );
            symlink(original.join("record"), temp.path().join("moved/link")).unwrap();
            assert!(child(&descriptor, "link", false, false).is_err());
            assert!(child(&descriptor, "record", true, false).is_err());
            assert!(domain_record(&original.join("record")).is_err());
            for path in ["relative", "/tmp/../record", "/tmp//record", "/tmp/record/"] {
                assert!(domain_record(Path::new(path)).is_err());
            }
            let file = File::open(original.join("record")).unwrap();
            assert!(bounded(file, 2).is_err());
        }

        #[test]
        #[ignore = "Run in the separate root-owned isolated fixture container."]
        fn trusted_root_record_checks_modes_types_bounds_and_ancestors() {
            assert_eq!(unsafe { libc::getuid() }, 0);
            assert!(Path::new("/.dockerenv").is_file());
            let temp = tempfile::Builder::new()
                .prefix("host-control-")
                .tempdir_in("/")
                .unwrap();
            let path = temp.path().join("record");
            fs::write(&path, b"{}").unwrap();
            assert_eq!(domain_record(&path).unwrap(), b"{}");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
            assert!(domain_record(&path).is_err());
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            fs::write(&path, vec![b'x'; RECORD_LIMIT as usize + 1]).unwrap();
            assert!(domain_record(&path).is_err());
            fs::write(&path, b"{}").unwrap();
            fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o777)).unwrap();
            assert!(domain_record(&path).is_err());
            fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
            fs::remove_file(&path).unwrap();
            let name = CString::new(path.to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            assert!(domain_record(&path).is_err());
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
