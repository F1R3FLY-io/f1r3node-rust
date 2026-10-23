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
    StopUnder {
        #[arg(long)]
        owner: String,
        directory: PathBuf,
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
    use std::time::{Duration, Instant, SystemTime};

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

    fn same_user(directory: &File) -> Result<bool> {
        let status = String::from_utf8(bounded(
            child(directory, "status", false, false)?,
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
        Ok(uids[1] == unsafe { libc::geteuid() })
    }

    fn pids() -> Result<Vec<i32>> {
        let entries = fs::read_dir("/proc")?.collect::<io::Result<Vec<_>>>()?;
        let mut pids: Vec<i32> = entries
            .iter()
            .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
            .collect();
        pids.sort_unstable();
        Ok(pids)
    }

    fn reaped<'a>(processes: impl Iterator<Item = &'a File>) -> bool {
        let deadline = Instant::now() + Duration::from_millis(750);
        let mut confirmed = true;
        for process in processes {
            loop {
                let remaining = deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis() as i32;
                match exited(process, remaining) {
                    Ok(true) => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted && remaining > 0 => {
                        continue
                    }
                    _ => {
                        confirmed = false;
                        break;
                    }
                }
            }
        }
        confirmed
    }

    fn owned(owner: &str) -> Result<(Vec<Owned>, bool)> {
        ensure!(
            !owner.is_empty() && owner.len() <= 256 && !owner.chars().any(char::is_control),
            "The owner marker is invalid."
        );
        let marker = format!("SOAK_PROCESS_OWNER={owner}");
        let mut result = Vec::new();
        let mut failed = false;
        for pid in pids()? {
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
                if !same_user(&directory)? {
                    return Ok(None);
                }
                let environment = match child(&directory, "environ", false, false)
                    .and_then(|file| bounded(file, 16 * 1024 * 1024))
                {
                    Ok(environment) => environment,
                    Err(error)
                        if matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)) =>
                    {
                        return Ok(None);
                    }
                    Err(error) => return Err(error.into()),
                };
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
        if !reaped(processes.iter().map(|owned| &owned.process)) {
            failed = true;
        }
        ensure!(!failed, "Writer termination is unconfirmed.");
        Ok(())
    }

    struct Residual {
        process: File,
        pid: i32,
        name: String,
        reason: &'static str,
        environment: &'static str,
        owner: &'static str,
    }

    fn names(argument: &[u8], root: &[u8]) -> bool {
        let value = match argument.iter().position(|byte| *byte == b'=') {
            Some(index) if argument.starts_with(b"-") => &argument[index + 1..],
            _ => argument,
        };
        value
            .strip_prefix(root)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(b"/"))
    }

    fn ancestors() -> Result<Vec<i32>> {
        let mut chain = Vec::new();
        let mut pid = std::process::id() as i32;
        loop {
            chain.push(pid);
            if pid <= 1 {
                break;
            }
            let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
            pid = stat
                .rsplit_once(") ")
                .ok_or_else(|| eyre!("The process identity is invalid."))?
                .1
                .split_whitespace()
                .nth(1)
                .ok_or_else(|| eyre!("The parent process is unavailable."))?
                .parse()?;
        }
        Ok(chain)
    }

    fn owner_state(directory: &File, owner: &str) -> Result<(&'static str, &'static str)> {
        match child(directory, "environ", false, false)
            .and_then(|file| bounded(file, 16 * 1024 * 1024))
        {
            Ok(environment) => {
                let marker = format!("SOAK_PROCESS_OWNER={owner}");
                let mut state = "absent";
                for value in environment.split(|byte| *byte == 0) {
                    if value == marker.as_bytes() {
                        return Ok(("readable", "match"));
                    }
                    if value.starts_with(b"SOAK_PROCESS_OWNER=") {
                        state = "other";
                    }
                }
                Ok(("readable", state))
            }
            Err(error) if matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)) => {
                Ok(("unreadable", "unknown"))
            }
            Err(error) => Err(error.into()),
        }
    }

    fn residual(root: &Path, owner: &str) -> Result<(Vec<Residual>, bool)> {
        let root = fs::canonicalize(root)?;
        let root = root.as_os_str().as_encoded_bytes();
        let excluded = ancestors()?;
        let mut result = Vec::new();
        let mut failed = false;
        for pid in pids()? {
            if excluded.contains(&pid) {
                continue;
            }
            let process = match open_pid(pid) {
                Ok(fd) => fd,
                Err(error) if error.raw_os_error() == Some(libc::ESRCH) => continue,
                Err(_) => {
                    failed = true;
                    continue;
                }
            };
            let scan = (|| -> Result<Option<Residual>> {
                let directory = directory(Path::new(&format!("/proc/{pid}")))?;
                if !same_user(&directory)? {
                    return Ok(None);
                }
                let arguments = bounded(child(&directory, "cmdline", false, false)?, 1024 * 1024)?;
                let reason = if arguments
                    .split(|byte| *byte == 0)
                    .any(|argument| names(argument, root))
                {
                    "arguments"
                } else {
                    match fs::read_link(format!("/proc/{pid}/cwd")) {
                        Ok(cwd) if names(cwd.as_os_str().as_encoded_bytes(), root) => "cwd",
                        Ok(_) => return Ok(None),
                        Err(error)
                            if matches!(error.raw_os_error(), Some(libc::EACCES | libc::EPERM)) =>
                        {
                            "uninspected"
                        }
                        Err(error) => return Err(error.into()),
                    }
                };
                let name = String::from_utf8_lossy(&bounded(
                    child(&directory, "comm", false, false)?,
                    RECORD_LIMIT,
                )?)
                .trim_end()
                .replace(char::is_whitespace, "_");
                let (environment, owner) = owner_state(&directory, owner)?;
                if exited(&process, 0)? {
                    return Ok(None);
                }
                Ok(Some(Residual {
                    process: process.try_clone()?,
                    pid,
                    name,
                    reason,
                    environment,
                    owner,
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

    fn stop_under(root: &Path, owner: &str, output: &mut impl Write) -> Result<()> {
        ensure!(
            !owner.is_empty() && owner.len() <= 256 && !owner.chars().any(char::is_control),
            "The owner marker is invalid."
        );
        if !root.is_dir() {
            return Ok(());
        }
        let (processes, mut failed) = residual(root, owner)?;
        let mut stopped = Vec::new();
        for residual in &processes {
            writeln!(
                output,
                "pid={} name={} reason={} environment={} owner={}",
                residual.pid, residual.name, residual.reason, residual.environment, residual.owner
            )?;
            if residual.reason == "uninspected" {
                continue;
            }
            if let Err(error) = signal(&residual.process, libc::SIGKILL) {
                if error.raw_os_error() != Some(libc::ESRCH) {
                    failed = true;
                }
            }
            stopped.push(&residual.process);
        }
        if !reaped(stopped.into_iter()) {
            failed = true;
        }
        if !quiet(root, QUIET_WINDOW)? {
            failed = true;
        }
        ensure!(!failed, "Writer termination is unconfirmed.");
        Ok(())
    }

    const QUIET_WINDOW: Duration = Duration::from_secs(2);

    fn newest(path: &Path) -> Result<Option<SystemTime>> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut latest = Some(metadata.modified()?);
        if metadata.is_dir() {
            let entries = match fs::read_dir(path) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(latest),
                Err(error) => return Err(error.into()),
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error.into()),
                };
                latest = latest.max(newest(&entry.path())?);
            }
        }
        Ok(latest)
    }

    fn quiet(root: &Path, window: Duration) -> Result<bool> {
        let before = newest(root)?;
        std::thread::sleep(window);
        Ok(newest(root)? <= before)
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
            Action::StopUnder { owner, directory } => {
                stop_under(&directory, &owner, &mut io::stdout().lock())?
            }
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

        struct Undumpable(libc::pid_t);
        impl Drop for Undumpable {
            fn drop(&mut self) {
                unsafe {
                    libc::kill(self.0, libc::SIGKILL);
                    libc::waitpid(self.0, std::ptr::null_mut(), 0);
                }
            }
        }
        fn undumpable() -> Undumpable {
            assert!(
                Path::new("/.dockerenv").is_file(),
                "Run process fixtures in an isolated container."
            );
            let mut pipe = [0; 2];
            assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
            let pid = unsafe { libc::fork() };
            assert!(pid >= 0);
            if pid == 0 {
                unsafe {
                    libc::close(pipe[0]);
                    libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
                    libc::write(pipe[1], b"r".as_ptr().cast(), 1);
                    loop {
                        libc::pause();
                    }
                }
            }
            let mut ready = [0u8; 1];
            unsafe {
                libc::close(pipe[1]);
                assert_eq!(libc::read(pipe[0], ready.as_mut_ptr().cast(), 1), 1);
                libc::close(pipe[0]);
            }
            Undumpable(pid)
        }

        fn unreadable_environment(pid: libc::pid_t) {
            let read = fs::read(format!("/proc/{pid}/environ"));
            if unsafe { libc::geteuid() } != 0 {
                assert!(matches!(
                    read.unwrap_err().raw_os_error(),
                    Some(libc::EACCES | libc::EPERM)
                ));
            }
        }

        #[test]
        fn stop_skips_a_process_whose_environment_is_unreadable() {
            let owner = marker();
            let hidden = undumpable();
            unreadable_environment(hidden.0);
            let mut first = process(&owner);
            stop(&owner).unwrap();
            assert!(first.0.wait().unwrap().code().is_none());
            assert_eq!(
                unsafe { libc::waitpid(hidden.0, std::ptr::null_mut(), libc::WNOHANG) },
                0
            );
        }

        #[test]
        fn oom_preference_skips_a_process_whose_environment_is_unreadable() {
            let owner = marker();
            let hidden = undumpable();
            unreadable_environment(hidden.0);
            let path = format!("/proc/{}/oom_score_adj", hidden.0);
            let before = fs::read_to_string(&path).unwrap();
            let original = process(&owner);
            prefer_oom(&owner).unwrap();
            assert_eq!(
                fs::read_to_string(format!("/proc/{}/oom_score_adj", original.0.id())).unwrap(),
                "1000\n"
            );
            assert_eq!(fs::read_to_string(path).unwrap(), before);
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

        fn named(owner: Option<&str>, argument: &str, cwd: &Path) -> Process {
            assert!(
                Path::new("/.dockerenv").is_file(),
                "Run process fixtures in an isolated container."
            );
            let mut command = Command::new("/bin/sh");
            command
                .args(["-c", "printf 'ready\\n'; read -r _", "fixture", argument])
                .current_dir(cwd)
                .env_remove("SOAK_PROCESS_OWNER")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
            if let Some(owner) = owner {
                command.env("SOAK_PROCESS_OWNER", owner);
            }
            let mut child = Process(command.spawn().unwrap());
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

        #[test]
        fn directory_names_require_a_path_boundary() {
            assert!(names(b"/soak/data", b"/soak/data"));
            assert!(names(b"/soak/data/s/v", b"/soak/data"));
            assert!(names(b"--data-dir=/soak/data/s/v", b"/soak/data"));
            assert!(!names(b"/soak/data-old/v", b"/soak/data"));
            assert!(!names(b"/soak", b"/soak/data"));
            assert!(!names(b"key=/soak/data/v", b"/soak/data"));
        }

        #[test]
        fn residual_scan_records_why_the_owner_stop_missed_a_writer() {
            let owner = marker();
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("data");
            let session = root.join("session");
            fs::create_dir_all(&session).unwrap();
            let outside = tempfile::tempdir().unwrap();
            let neutral = Path::new("/");
            let text = |path: &Path| path.to_str().unwrap().to_owned();
            let mut marked = named(Some(&owner), &text(&session.join("validator1")), neutral);
            let mut unmarked = named(
                None,
                &format!("--data-dir={}", text(&session.join("v2"))),
                neutral,
            );
            let mut other = named(Some("other"), "plain", &session);
            let mut sibling = named(None, &format!("{}-old/v", text(&root)), neutral);
            let mut unrelated = named(None, &text(outside.path()), neutral);
            let (found, failed) = residual(&root, &owner).unwrap();
            assert!(!failed);
            let state = |child: &Process| {
                found
                    .iter()
                    .find(|residual| residual.pid == child.0.id() as i32)
                    .map(|residual| (residual.reason, residual.environment, residual.owner))
            };
            assert_eq!(state(&marked), Some(("arguments", "readable", "match")));
            assert_eq!(state(&unmarked), Some(("arguments", "readable", "absent")));
            assert_eq!(state(&other), Some(("cwd", "readable", "other")));
            assert_eq!(state(&sibling), None);
            assert_eq!(state(&unrelated), None);
            assert_eq!(found.len(), 3);
            let mut record = Vec::new();
            stop_under(&root, &owner, &mut record).unwrap();
            let record = String::from_utf8(record).unwrap();
            assert_eq!(record.lines().count(), 3);
            assert!(record.contains(&format!(
                "pid={} name=sh reason=arguments environment=readable owner=absent",
                unmarked.0.id()
            )));
            assert!(marked.0.wait().unwrap().code().is_none());
            assert!(unmarked.0.wait().unwrap().code().is_none());
            assert!(other.0.wait().unwrap().code().is_none());
            assert!(sibling.0.try_wait().unwrap().is_none());
            assert!(unrelated.0.try_wait().unwrap().is_none());
        }

        #[test]
        fn residual_scan_classifies_an_unreadable_environment() {
            let hidden = undumpable();
            unreadable_environment(hidden.0);
            let directory = directory(Path::new(&format!("/proc/{}", hidden.0))).unwrap();
            let state = owner_state(&directory, &marker()).unwrap();
            match fs::read(format!("/proc/{}/environ", hidden.0)) {
                Ok(_) => assert_eq!(state, ("readable", "absent")),
                Err(_) => assert_eq!(state, ("unreadable", "unknown")),
            }
        }

        fn hidden_writer(root: &Path) -> Undumpable {
            assert!(
                Path::new("/.dockerenv").is_file(),
                "Run process fixtures in an isolated container."
            );
            let directory = CString::new(root.to_str().unwrap()).unwrap();
            let log = CString::new("writer.log").unwrap();
            let mut pipe = [0; 2];
            assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
            let pid = unsafe { libc::fork() };
            assert!(pid >= 0);
            if pid == 0 {
                unsafe {
                    libc::close(pipe[0]);
                    libc::chdir(directory.as_ptr());
                    libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
                    let fd = libc::open(
                        log.as_ptr(),
                        libc::O_WRONLY | libc::O_APPEND | libc::O_CREAT,
                        0o600,
                    );
                    libc::write(pipe[1], b"r".as_ptr().cast(), 1);
                    loop {
                        libc::write(fd, b"x".as_ptr().cast(), 1);
                        libc::usleep(100_000);
                    }
                }
            }
            let mut ready = [0u8; 1];
            unsafe {
                libc::close(pipe[1]);
                assert_eq!(libc::read(pipe[0], ready.as_mut_ptr().cast(), 1), 1);
                libc::close(pipe[0]);
            }
            Undumpable(pid)
        }

        #[test]
        fn residual_stop_fails_when_an_uninspectable_writer_keeps_writing() {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("data");
            fs::create_dir_all(&root).unwrap();
            let hidden = hidden_writer(&root);
            let inspectable = fs::read_link(format!("/proc/{}/cwd", hidden.0)).is_ok();
            let mut record = Vec::new();
            let result = stop_under(&root, &marker(), &mut record);
            let record = String::from_utf8(record).unwrap();
            if inspectable {
                result.unwrap();
                assert!(record.contains(&format!("pid={} ", hidden.0)));
                assert!(record.contains("reason=cwd"));
            } else {
                assert!(result.is_err());
                assert!(record.lines().any(|line| {
                    line.starts_with(&format!("pid={} ", hidden.0))
                        && line.contains("reason=uninspected")
                }));
                assert_eq!(
                    unsafe { libc::waitpid(hidden.0, std::ptr::null_mut(), libc::WNOHANG) },
                    0
                );
            }
        }

        #[test]
        fn residual_stop_accepts_an_uninspectable_process_that_writes_elsewhere() {
            let hidden = undumpable();
            unreadable_environment(hidden.0);
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("data");
            fs::create_dir_all(root.join("session")).unwrap();
            fs::write(root.join("session/validator1.log"), b"old").unwrap();
            let mut record = Vec::new();
            stop_under(&root, &marker(), &mut record).unwrap();
            assert_eq!(
                unsafe { libc::waitpid(hidden.0, std::ptr::null_mut(), libc::WNOHANG) },
                0
            );
        }

        #[test]
        fn residual_stop_excludes_its_ancestors_and_accepts_a_missing_directory() {
            let chain = ancestors().unwrap();
            assert!(chain.contains(&(std::process::id() as i32)));
            assert!(chain.contains(&(unsafe { libc::getppid() })));
            let temp = tempfile::tempdir().unwrap();
            let mut record = Vec::new();
            stop_under(&temp.path().join("absent"), &marker(), &mut record).unwrap();
            assert!(record.is_empty());
        }
    }
}
