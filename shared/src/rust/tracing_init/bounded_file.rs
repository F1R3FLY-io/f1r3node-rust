use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{LogFileConfig, LogRotation};

struct RotatedFile {
    path: PathBuf,
    bytes: u64,
}

pub(super) struct BoundedFile {
    directory: PathBuf,
    config: LogFileConfig,
    file: Option<File>,
    current_bytes: u64,
    total_bytes: u64,
    rotated: VecDeque<RotatedFile>,
    period: Option<u64>,
    sequence: u128,
    _lock: File,
}

impl BoundedFile {
    pub(super) fn new(data_dir: &Path, config: &LogFileConfig) -> io::Result<Self> {
        Self::new_at(data_dir, config, SystemTime::now())
    }

    fn new_at(data_dir: &Path, config: &LogFileConfig, now: SystemTime) -> io::Result<Self> {
        config.validate()?;
        fs::create_dir_all(data_dir)?;
        let lock_path = data_dir.join(".node-log.lock");
        if let Ok(metadata) = fs::symlink_metadata(&lock_path) {
            if !metadata.file_type().is_file() {
                return Err(io::Error::other("The node log lock is not a regular file."));
            }
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock.try_lock().map_err(io::Error::other)?;

        let directory = data_dir.join("logs");
        if let Ok(metadata) = fs::symlink_metadata(&directory) {
            if !metadata.file_type().is_dir() {
                return Err(io::Error::other("The log directory is not a directory."));
            }
        }
        fs::create_dir_all(&directory)?;
        let mut entries = Vec::new();
        let mut unmanaged_bytes = 0_u64;
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let metadata = fs::symlink_metadata(entry.path())?;
            if !metadata.file_type().is_file() {
                return Err(io::Error::other(
                    "The log directory contains an unsupported entry.",
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() != 1 {
                    return Err(io::Error::other(
                        "The log directory contains a hard-linked file.",
                    ));
                }
            }
            let name = entry.file_name();
            let managed = name
                .to_str()
                .is_some_and(|name| name == "node.log" || managed_archive(name));
            if !managed {
                unmanaged_bytes = unmanaged_bytes
                    .checked_add(metadata.len())
                    .ok_or_else(|| io::Error::other("The log directory byte count overflowed."))?;
            }
            entries.push((metadata.modified()?, entry.path(), metadata.len(), managed));
        }
        if unmanaged_bytes > config.max_total_size_bytes {
            return Err(io::Error::other(
                "Unmanaged log directory files exceed the byte budget.",
            ));
        }
        entries.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
        let mut current_bytes = 0;
        let mut current_time = now;
        let mut total_bytes = unmanaged_bytes;
        let mut rotated = VecDeque::new();
        for (modified, path, bytes, managed) in entries {
            if !managed {
                continue;
            }
            if bytes > config.max_file_size_bytes {
                fs::remove_file(path)?;
                continue;
            }
            total_bytes = total_bytes
                .checked_add(bytes)
                .ok_or_else(|| io::Error::other("The log directory byte count overflowed."))?;
            if path.file_name().is_some_and(|name| name == "node.log") {
                current_bytes = bytes;
                current_time = modified;
            } else {
                rotated.push_back(RotatedFile { path, bytes });
            }
        }
        let mut writer = Self {
            directory,
            config: config.clone(),
            file: None,
            current_bytes,
            total_bytes,
            rotated,
            period: period(config.rotation, current_time)?,
            sequence: now
                .duration_since(UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos(),
            _lock: lock,
        };
        writer.make_room(0)?;
        if writer.file.is_none() {
            writer.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(writer.directory.join("node.log"))?,
            );
        }
        Ok(writer)
    }

    fn rotate(&mut self, next_period: Option<u64>) -> io::Result<()> {
        if self.current_bytes == 0 {
            self.period = next_period;
            return Ok(());
        }
        let active = self.directory.join("node.log");
        let destination = loop {
            let candidate = self
                .directory
                .join(format!("node.log.{:020}", self.sequence));
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| io::Error::other("The log rotation sequence overflowed."))?;
            match fs::symlink_metadata(&candidate) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => break candidate,
                Ok(_) => continue,
                Err(error) => return Err(error),
            }
        };
        drop(self.file.take());
        fs::rename(active, &destination)?;
        self.rotated.push_back(RotatedFile {
            path: destination,
            bytes: self.current_bytes,
        });
        self.current_bytes = 0;
        self.file = Some(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.directory.join("node.log"))?,
        );
        self.period = next_period;
        Ok(())
    }

    fn make_room(&mut self, bytes: u64) -> io::Result<()> {
        let available = self
            .config
            .max_total_size_bytes
            .checked_sub(bytes)
            .ok_or_else(|| io::Error::other("The write exceeds the log directory byte budget."))?;
        while self.total_bytes > available
            || (self.config.retention > 0 && self.rotated.len() > self.config.retention)
        {
            if self.rotated.is_empty() {
                if self.current_bytes == 0 {
                    return Err(io::Error::other(
                        "Unmanaged files leave no space in the log byte budget.",
                    ));
                }
                self.rotate(self.period)?;
            }
            let oldest = self
                .rotated
                .front()
                .ok_or_else(|| io::Error::other("The rotated log inventory is unavailable."))?;
            fs::remove_file(&oldest.path)?;
            self.total_bytes -= oldest.bytes;
            self.rotated.pop_front();
        }
        Ok(())
    }

    fn write_at(&mut self, bytes: &[u8], now: SystemTime) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        if self.file.is_none() {
            return Err(io::Error::other("The active log file is unavailable."));
        }
        let next_period = period(self.config.rotation, now)?;
        let input_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let remaining = self.config.max_file_size_bytes - self.current_bytes;
        if next_period != self.period
            || remaining == 0
            || (input_bytes <= self.config.max_file_size_bytes && input_bytes > remaining)
        {
            self.rotate(next_period)?;
        }
        let count = bytes.len().min(
            usize::try_from(self.config.max_file_size_bytes - self.current_bytes)
                .unwrap_or(usize::MAX),
        );
        self.make_room(count as u64)?;
        let written = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("The active log file is unavailable."))?
            .write(&bytes[..count])?;
        self.current_bytes += written as u64;
        self.total_bytes += written as u64;
        Ok(written)
    }
}

impl Write for BoundedFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.write_at(bytes, SystemTime::now())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("The active log file is unavailable."))?
            .flush()
    }
}

fn period(rotation: LogRotation, now: SystemTime) -> io::Result<Option<u64>> {
    let seconds = match rotation {
        LogRotation::Never => return Ok(None),
        LogRotation::Minutely => 60,
        LogRotation::Hourly => 3600,
        LogRotation::Daily => 86400,
    };
    Ok(Some(
        now.duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs()
            / seconds,
    ))
}

fn managed_archive(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix("node.log.") else {
        return false;
    };
    if suffix.len() == 20 && suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    let parts: Vec<_> = suffix.split('-').collect();
    (3..=5).contains(&parts.len())
        && parts.iter().enumerate().all(|(index, part)| {
            part.len() == if index == 0 { 4 } else { 2 }
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use proptest::prelude::*;
    use tempfile::tempdir;

    use super::*;

    fn config(file: u64, total: u64) -> LogFileConfig {
        LogFileConfig {
            max_file_size_bytes: file,
            max_total_size_bytes: total,
            ..LogFileConfig::default()
        }
    }

    fn assert_bounds(path: &Path, cfg: &LogFileConfig) {
        let sizes: Vec<_> = fs::read_dir(path.join("logs"))
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .collect();
        assert!(
            sizes.iter().all(|size| *size <= cfg.max_file_size_bytes),
            "{sizes:?}"
        );
        assert!(
            sizes.iter().sum::<u64>() <= cfg.max_total_size_bytes,
            "{sizes:?}"
        );
    }

    #[test]
    fn sustained_writes_never_exceed_either_byte_limit() {
        for rotation in [
            LogRotation::Never,
            LogRotation::Minutely,
            LogRotation::Hourly,
            LogRotation::Daily,
        ] {
            for retention in [0, 1, 3] {
                let root = tempdir().unwrap();
                let cfg = LogFileConfig {
                    rotation,
                    retention,
                    ..config(64, 192)
                };
                let mut writer = BoundedFile::new(root.path(), &cfg).unwrap();
                for _ in 0..200 {
                    writer
                        .write_all(b"ERROR repeated accept failure\n")
                        .unwrap();
                    assert_bounds(root.path(), &cfg);
                    if retention > 0 {
                        assert!(writer.rotated.len() <= retention);
                    }
                }
                writer.write_all(b"FINAL").unwrap();
                writer.flush().unwrap();
                assert!(fs::read(root.path().join("logs/node.log"))
                    .unwrap()
                    .ends_with(b"FINAL"));
                assert_bounds(root.path(), &cfg);
            }
        }
    }

    #[test]
    fn an_oversized_write_spans_files_without_losing_bytes() {
        let root = tempdir().unwrap();
        let cfg = config(17, 1024);
        let mut writer = BoundedFile::new(root.path(), &cfg).unwrap();
        let input: Vec<_> = (0..511).map(|index| (index % 256) as u8).collect();
        writer.write_all(&input).unwrap();
        writer.flush().unwrap();
        let mut output = Vec::new();
        for file in &writer.rotated {
            output.extend(fs::read(&file.path).unwrap());
        }
        output.extend(fs::read(root.path().join("logs/node.log")).unwrap());
        assert_eq!(input, output);
        assert_bounds(root.path(), &cfg);
    }

    #[test]
    fn oldest_rotated_file_is_removed_before_total_budget_is_exceeded() {
        let root = tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        let oldest = logs.join("node.log.2026-01-01");
        let newer = logs.join("node.log.2026-01-02");
        fs::write(&oldest, [1; 16]).unwrap();
        fs::write(&newer, [2; 16]).unwrap();
        fs::write(logs.join("node.log"), [3; 12]).unwrap();
        let cfg = config(16, 48);
        let mut writer = BoundedFile::new(root.path(), &cfg).unwrap();
        writer.write_all(&[4; 4]).unwrap();
        assert!(oldest.exists());
        writer.write_all(&[5; 5]).unwrap();
        assert!(!oldest.exists());
        assert!(newer.exists());
        assert_eq!(fs::read(logs.join("node.log")).unwrap(), [5; 5]);
        assert_bounds(root.path(), &cfg);
    }

    #[test]
    fn a_record_within_the_file_limit_is_not_split_at_a_rotation_boundary() {
        let root = tempdir().unwrap();
        let mut writer = BoundedFile::new(root.path(), &config(16, 64)).unwrap();
        writer.write_all(b"first record\n").unwrap();
        writer.write_all(b"next record\n").unwrap();
        assert_eq!(writer.rotated.len(), 1);
        assert_eq!(
            fs::read(&writer.rotated[0].path).unwrap(),
            b"first record\n"
        );
        assert_eq!(
            fs::read(root.path().join("logs/node.log")).unwrap(),
            b"next record\n"
        );
    }

    #[test]
    fn restart_rotates_an_active_file_from_an_earlier_period() {
        let root = tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        let active = logs.join("node.log");
        fs::write(&active, b"previous").unwrap();
        let before = UNIX_EPOCH + Duration::from_secs(86400 * 20000);
        File::options()
            .write(true)
            .open(&active)
            .unwrap()
            .set_modified(before)
            .unwrap();
        let cfg = LogFileConfig {
            rotation: LogRotation::Daily,
            ..config(32, 128)
        };
        let after = before + Duration::from_secs(86400);
        let mut writer = BoundedFile::new_at(root.path(), &cfg, after).unwrap();
        writer.write_at(b"current", after).unwrap();
        assert_eq!(writer.rotated.len(), 1);
        assert_eq!(fs::read(&writer.rotated[0].path).unwrap(), b"previous");
        assert_eq!(fs::read(&active).unwrap(), b"current");
    }

    #[test]
    fn an_equal_file_and_directory_budget_is_supported() {
        let root = tempdir().unwrap();
        let cfg = config(64, 64);
        let mut writer = BoundedFile::new(root.path(), &cfg).unwrap();
        for _ in 0..20 {
            writer.write_all(&[1; 47]).unwrap();
            assert_bounds(root.path(), &cfg);
        }
    }

    #[test]
    fn restart_enforces_smaller_limits_and_removes_oversized_legacy_logs() {
        let root = tempdir().unwrap();
        {
            let mut writer = BoundedFile::new(root.path(), &config(64, 256)).unwrap();
            writer.write_all(&[1; 200]).unwrap();
        }
        let logs = root.path().join("logs");
        fs::write(logs.join("node.log.2026-01-01-12-30"), [2; 65]).unwrap();
        let cfg = config(16, 32);
        let mut writer = BoundedFile::new(root.path(), &cfg).unwrap();
        assert_bounds(root.path(), &cfg);
        writer.write_all(b"latest").unwrap();
        assert_bounds(root.path(), &cfg);
        assert!(!logs.join("node.log.2026-01-01-12-30").exists());
    }

    #[test]
    fn period_rotation_is_preserved_and_never_still_rotates_by_size() {
        for (rotation, seconds) in [
            (LogRotation::Minutely, 60),
            (LogRotation::Hourly, 3600),
            (LogRotation::Daily, 86400),
        ] {
            let root = tempdir().unwrap();
            let cfg = LogFileConfig {
                rotation,
                ..config(32, 128)
            };
            let now = UNIX_EPOCH + Duration::from_secs(86400 * 20000);
            let mut writer = BoundedFile::new_at(root.path(), &cfg, now).unwrap();
            writer.write_at(b"before", now).unwrap();
            writer
                .write_at(b"same", now + Duration::from_secs(seconds - 1))
                .unwrap();
            assert!(writer.rotated.is_empty());
            writer
                .write_at(b"after", now + Duration::from_secs(seconds))
                .unwrap();
            assert_eq!(writer.rotated.len(), 1);
            assert_eq!(fs::read(&writer.rotated[0].path).unwrap(), b"beforesame");
            assert_eq!(
                fs::read(root.path().join("logs/node.log")).unwrap(),
                b"after"
            );
        }
    }

    #[test]
    fn zero_or_inverted_byte_limits_are_rejected() {
        let root = tempdir().unwrap();
        for cfg in [config(0, 0), config(0, 32), config(32, 0), config(33, 32)] {
            assert!(BoundedFile::new(root.path(), &cfg).is_err());
        }
        assert!(!root.path().join("logs").exists());
    }

    #[test]
    fn legacy_configuration_receives_bounded_defaults() {
        let cfg: LogFileConfig =
            serde_json::from_str(r#"{"rotation":"daily","retention":14}"#).unwrap();
        assert_eq!(cfg.max_file_size_bytes, 100 * 1024 * 1024);
        assert_eq!(cfg.max_total_size_bytes, 2 * 1024 * 1024 * 1024);
        cfg.validate().unwrap();
        let cfg: LogFileConfig = serde_json::from_str("{}").unwrap();
        cfg.validate().unwrap();
        for field in ["max-file-size-bytes", "max-total-size-bytes"] {
            for value in ["-1", "null", "1.5", "\"64\""] {
                let input = format!("{{\"{field}\":{value}}}");
                assert!(serde_json::from_str::<LogFileConfig>(&input).is_err());
            }
        }
    }

    #[test]
    fn only_one_cooperating_writer_can_hold_the_log_directory() {
        let root = tempdir().unwrap();
        let cfg = config(32, 64);
        let first = BoundedFile::new(root.path(), &cfg).unwrap();
        assert!(BoundedFile::new(root.path(), &cfg).is_err());
        drop(first);
        assert!(BoundedFile::new(root.path(), &cfg).is_ok());
    }

    #[test]
    fn unrelated_regular_files_are_preserved_and_counted() {
        let root = tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        let unrelated = logs.join("operator-note.txt");
        fs::write(&unrelated, [9; 12]).unwrap();
        let cfg = config(16, 32);
        let mut writer = BoundedFile::new(root.path(), &cfg).unwrap();
        for _ in 0..20 {
            writer.write_all(&[1; 15]).unwrap();
            assert_bounds(root.path(), &cfg);
        }
        assert_eq!(fs::read(&unrelated).unwrap(), [9; 12]);
    }

    #[test]
    fn unrelated_files_that_exhaust_the_budget_prevent_growth() {
        let root = tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        let unrelated = logs.join("operator-note.txt");
        fs::write(&unrelated, [9; 32]).unwrap();
        let mut writer = BoundedFile::new(root.path(), &config(16, 32)).unwrap();
        assert!(writer.write_all(b"failure").is_err());
        assert_eq!(fs::metadata(logs.join("node.log")).unwrap().len(), 0);
        assert_eq!(fs::read(&unrelated).unwrap(), [9; 32]);
        drop(writer);
        fs::write(&unrelated, [9; 33]).unwrap();
        assert!(BoundedFile::new(root.path(), &config(16, 32)).is_err());
    }

    #[test]
    fn unsupported_entries_are_rejected_before_legacy_logs_are_removed() {
        let root = tempdir().unwrap();
        let logs = root.path().join("logs");
        fs::create_dir(&logs).unwrap();
        fs::write(logs.join("node.log"), [1; 100]).unwrap();
        fs::create_dir(logs.join("unexpected-directory")).unwrap();
        assert!(BoundedFile::new(root.path(), &config(16, 32)).is_err());
        assert_eq!(fs::metadata(logs.join("node.log")).unwrap().len(), 100);
    }

    #[test]
    fn failed_eviction_prevents_the_next_write() {
        let root = tempdir().unwrap();
        let cfg = config(16, 32);
        let mut writer = BoundedFile::new(root.path(), &cfg).unwrap();
        writer.write_all(&[1; 32]).unwrap();
        let oldest = writer.rotated[0].path.clone();
        fs::remove_file(&oldest).unwrap();
        fs::create_dir(&oldest).unwrap();
        assert!(writer.write_all(b"must not grow").is_err());
        assert_eq!(
            fs::metadata(root.path().join("logs/node.log"))
                .unwrap()
                .len(),
            0
        );
        assert!(oldest.is_dir());
    }

    #[test]
    fn empty_writes_do_not_create_rotated_files() {
        let root = tempdir().unwrap();
        let mut writer = BoundedFile::new(root.path(), &config(16, 32)).unwrap();
        assert_eq!(writer.write(&[]).unwrap(), 0);
        assert!(writer.rotated.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_hard_links_are_rejected_without_touching_targets() {
        use std::os::unix::fs::symlink;

        for link_name in ["node.log", "node.log.2026-01-01", "operator-note.txt"] {
            let root = tempdir().unwrap();
            let outside = root.path().join("outside");
            fs::write(&outside, b"preserve").unwrap();
            let logs = root.path().join("logs");
            fs::create_dir(&logs).unwrap();
            symlink(&outside, logs.join(link_name)).unwrap();
            assert!(BoundedFile::new(root.path(), &config(16, 32)).is_err());
            assert_eq!(fs::read(&outside).unwrap(), b"preserve");
        }
        let root = tempdir().unwrap();
        fs::write(root.path().join("outside"), b"preserve").unwrap();
        fs::create_dir(root.path().join("logs")).unwrap();
        fs::hard_link(
            root.path().join("outside"),
            root.path().join("logs/node.log"),
        )
        .unwrap();
        assert!(BoundedFile::new(root.path(), &config(16, 32)).is_err());
        assert_eq!(fs::read(root.path().join("outside")).unwrap(), b"preserve");
    }

    #[cfg(unix)]
    #[test]
    fn directory_and_lock_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;

        for path in ["logs", ".node-log.lock"] {
            let root = tempdir().unwrap();
            let outside = root.path().join("outside");
            if path == "logs" {
                fs::create_dir(&outside).unwrap();
            } else {
                fs::write(&outside, b"preserve").unwrap();
            }
            symlink(&outside, root.path().join(path)).unwrap();
            assert!(BoundedFile::new(root.path(), &config(16, 32)).is_err());
        }
    }

    proptest! {
        #[test]
        fn arbitrary_write_lengths_remain_bounded(lengths in prop::collection::vec(1_usize..2048, 1..40)) {
            let root = tempdir().unwrap();
            let cfg = config(64, 192);
            let mut writer = BoundedFile::new(root.path(), &cfg).unwrap();
            for length in lengths {
                writer.write_all(&vec![1; length]).unwrap();
                assert_bounds(root.path(), &cfg);
            }
        }
    }
}
