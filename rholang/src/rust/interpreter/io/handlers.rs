// The 22 native filesystem handlers.
//
// Each handler:
//   1. Unapplies the incoming contract call to extract `(produce, is_replay, previous_output, args)`.
//   2. On replay, immediately re-sends `previous_output` — filesystem calls
//      are non-deterministic and must not be re-issued.
//   3. Otherwise validates the arguments, dispatches to `std::fs`, and
//      builds the `[true, ...]` / `[false, code, msg]` reply.
//
// The handlers are methods on `FsProcesses` so they share a single
// `FileHandleTable` across the runtime.

use std::path::{Path, PathBuf};

use models::rhoapi::{ListParWithRandom, Par};

use super::super::contract_call::ContractCall;
use super::super::dispatch::RhoDispatch;
use super::super::errors::{illegal_argument_error, InterpreterError};
use super::super::rho_runtime::RhoISpace;
use super::super::rho_type::{RhoByteArray, RhoNumber, RhoString};
use super::errors::*;
use super::handle_table::{FileHandle, FileHandleTable};
use super::mode::{open_options, parse_open_mode, AccessMode};
use super::path::{canonicalize_and_quarantine, quarantine_err_reply};
use super::response::*;
use super::stat::{error_record, stat_record};
use super::ConsensusMode;

/// Shared per-runtime state for the fs native handlers.  Cloned into
/// each handler closure via `ProcessContext`.
#[derive(Clone)]
pub struct FsProcesses {
    pub dispatcher: RhoDispatch,
    pub space: RhoISpace,
    pub handles: FileHandleTable,
    pub mode: ConsensusMode,
}

impl FsProcesses {
    pub fn new(
        dispatcher: RhoDispatch,
        space: RhoISpace,
        handles: FileHandleTable,
        mode: ConsensusMode,
    ) -> Self {
        FsProcesses {
            dispatcher,
            space,
            handles,
            mode,
        }
    }

    fn is_contract_call(&self) -> ContractCall {
        ContractCall {
            space: self.space.clone(),
            dispatcher: self.dispatcher.clone(),
        }
    }

    // -------------------------------------------------------------------
    // open — (rootCanon, rel, mode) -> [true, fd] | [false, code, msg]
    // -------------------------------------------------------------------
    pub async fn fs_open(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_open"));
        };
        let [root_par, rel_par, mode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_open"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoString::unapply(mode_par),
        ) {
            (Some(root), Some(rel), Some(mode_str)) => self.open_impl(&root, &rel, &mode_str).await,
            _ => err(FSERR_BAD_ARG, "expected (String, String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    async fn open_impl(&self, root: &str, rel: &str, mode: &str) -> Par {
        let intent = match parse_open_mode(mode) {
            Some(i) => i,
            None => return err(FSERR_BAD_ARG, format!("unknown fopen mode {mode:?}")),
        };
        let canon = match canonicalize_and_quarantine(Path::new(root), rel) {
            Ok(p) => p,
            Err(e) => {
                let (code, msg) = quarantine_err_reply(&e);
                return err(code, msg);
            }
        };

        // Reject non-regular files with FSERR_UNSUPPORTED.  We use
        // symlink_metadata to reflect the target directly — the quarantine
        // step already rejected any symlink components on the path.
        if let Ok(meta) = std::fs::symlink_metadata(&canon) {
            if !meta.file_type().is_file() {
                return err(FSERR_UNSUPPORTED, "not a regular file");
            }
        }

        let file = match open_options(intent).open(&canon) {
            Ok(f) => f,
            Err(e) => return err(io_err_code(&e), e.to_string()),
        };

        let handle = FileHandle {
            file,
            canon_path: canon,
            mode: intent.mode,
        };
        match self.handles.insert(handle).await {
            Ok(fd) => ok_u64(fd),
            Err(()) => err(FSERR_QUOTA_EXCEEDED, "per-runtime fd cap reached"),
        }
    }

    // -------------------------------------------------------------------
    // close — (fd) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_close(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_close"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_close"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) if fd >= 0 => {
                self.handles.remove(fd as u64).await;
                ok_bare()
            }
            _ => err(FSERR_BAD_ARG, "expected non-negative fd"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // read — (fd, n) -> [true, bytes]
    // -------------------------------------------------------------------
    pub async fn fs_read(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_read"));
        };
        let [fd_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_read"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if fd >= 0 && n >= 0 => {
                self.read_impl(fd as u64, n as u64, None).await
            }
            _ => err(FSERR_BAD_ARG, "expected (u64, u64)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // readAt — (fd, offset, n) -> [true, bytes]
    // -------------------------------------------------------------------
    pub async fn fs_read_at(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_read_at"));
        };
        let [fd_par, off_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_read_at"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoNumber::unapply(n_par),
        ) {
            (Some(fd), Some(off), Some(n)) if fd >= 0 && off >= 0 && n >= 0 => {
                self.read_impl(fd as u64, n as u64, Some(off as u64)).await
            }
            _ => err(FSERR_BAD_ARG, "expected (u64, u64, u64)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    async fn read_impl(&self, fd: u64, n: u64, offset: Option<u64>) -> Par {
        if n > super::MAX_READ_BYTES {
            return err(
                FSERR_QUOTA_EXCEEDED,
                format!("read {n} exceeds MAX_READ_BYTES"),
            );
        }
        let result: Option<std::io::Result<Vec<u8>>> = self
            .handles
            .with_mut(fd, |h| {
                let mut buf = vec![0u8; n as usize];
                use std::io::{Read, Seek, SeekFrom};
                let read_result = if let Some(off) = offset {
                    // Positional: save current position, seek+read, restore.
                    let cur = h.file.stream_position()?;
                    h.file.seek(SeekFrom::Start(off))?;
                    let r = h.file.read(&mut buf);
                    let _ = h.file.seek(SeekFrom::Start(cur));
                    r
                } else {
                    h.file.read(&mut buf)
                };
                read_result.map(|got| {
                    buf.truncate(got);
                    buf
                })
            })
            .await;
        match result {
            None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
            Some(Err(e)) => err(io_err_code(&e), e.to_string()),
            Some(Ok(bytes)) => ok_bytes(bytes),
        }
    }

    // -------------------------------------------------------------------
    // write — (fd, bytes) -> [true, nWritten]
    // -------------------------------------------------------------------
    pub async fn fs_write(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_write"));
        };
        let [fd_par, bytes_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_write"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoNumber::unapply(fd_par), RhoByteArray::unapply(bytes_par)) {
            (Some(fd), Some(bytes)) if fd >= 0 => self.write_impl(fd as u64, &bytes, None).await,
            _ => err(FSERR_BAD_ARG, "expected (u64, ByteArray)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // writeAt — (fd, offset, bytes) -> [true, nWritten]
    // -------------------------------------------------------------------
    pub async fn fs_write_at(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_write_at"));
        };
        let [fd_par, off_par, bytes_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_write_at"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoByteArray::unapply(bytes_par),
        ) {
            (Some(fd), Some(off), Some(bytes)) if fd >= 0 && off >= 0 => {
                self.write_impl(fd as u64, &bytes, Some(off as u64)).await
            }
            _ => err(FSERR_BAD_ARG, "expected (u64, u64, ByteArray)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    async fn write_impl(&self, fd: u64, bytes: &[u8], offset: Option<u64>) -> Par {
        let bytes_owned: Vec<u8> = bytes.to_vec();
        let result: Option<std::io::Result<usize>> = self
            .handles
            .with_mut(fd, move |h| {
                use std::io::{Seek, SeekFrom, Write};
                if let Some(off) = offset {
                    let cur = h.file.stream_position()?;
                    h.file.seek(SeekFrom::Start(off))?;
                    let r = h.file.write(&bytes_owned);
                    let _ = h.file.seek(SeekFrom::Start(cur));
                    r
                } else {
                    h.file.write(&bytes_owned)
                }
            })
            .await;
        match result {
            None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
            Some(Err(e)) => err(io_err_code(&e), e.to_string()),
            Some(Ok(n)) => ok_u64(n as u64),
        }
    }

    // -------------------------------------------------------------------
    // seek — (fd, offset, whence) -> [true, newPos]
    // -------------------------------------------------------------------
    pub async fn fs_seek(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_seek"));
        };
        let [fd_par, off_par, whence_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_seek"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoNumber::unapply(fd_par),
            RhoNumber::unapply(off_par),
            RhoString::unapply(whence_par),
        ) {
            (Some(fd), Some(off), Some(w)) if fd >= 0 => {
                use std::io::{Seek, SeekFrom};
                let whence = match w.as_str() {
                    "set" if off >= 0 => Some(SeekFrom::Start(off as u64)),
                    "cur" => Some(SeekFrom::Current(off)),
                    "end" => Some(SeekFrom::End(off)),
                    _ => None,
                };
                match whence {
                    None => err(FSERR_BAD_ARG, "expected whence in {set,cur,end}"),
                    Some(from) => {
                        let r = self
                            .handles
                            .with_mut(fd as u64, |h| h.file.seek(from))
                            .await;
                        match r {
                            None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                            Some(Err(e)) => err(io_err_code(&e), e.to_string()),
                            Some(Ok(pos)) => ok_u64(pos),
                        }
                    }
                }
            }
            _ => err(FSERR_BAD_ARG, "expected (u64, i64, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // tell — (fd) -> [true, pos]
    // -------------------------------------------------------------------
    pub async fn fs_tell(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_tell"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_tell"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) if fd >= 0 => {
                let r = self
                    .handles
                    .with_mut(fd as u64, |h| {
                        use std::io::Seek;
                        h.file.stream_position()
                    })
                    .await;
                match r {
                    None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                    Some(Err(e)) => err(io_err_code(&e), e.to_string()),
                    Some(Ok(pos)) => ok_u64(pos),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // size — (fd) -> [true, nBytes]
    // -------------------------------------------------------------------
    pub async fn fs_size(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_size"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_size"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) if fd >= 0 => {
                let r = self
                    .handles
                    .with_mut(fd as u64, |h| h.file.metadata().map(|m| m.len()))
                    .await;
                match r {
                    None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                    Some(Err(e)) => err(io_err_code(&e), e.to_string()),
                    Some(Ok(n)) => ok_u64(n),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // truncate — (fd, n) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_truncate(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_truncate"));
        };
        let [fd_par, n_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_truncate"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoNumber::unapply(fd_par), RhoNumber::unapply(n_par)) {
            (Some(fd), Some(n)) if fd >= 0 && n >= 0 => {
                if n as u64 > super::MAX_TRUNCATE_BYTES {
                    err(
                        FSERR_QUOTA_EXCEEDED,
                        format!("truncate {n} exceeds MAX_TRUNCATE_BYTES"),
                    )
                } else {
                    let r = self
                        .handles
                        .with_mut(fd as u64, |h| h.file.set_len(n as u64))
                        .await;
                    match r {
                        None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                        Some(Err(e)) => err(io_err_code(&e), e.to_string()),
                        Some(Ok(())) => ok_bare(),
                    }
                }
            }
            _ => err(FSERR_BAD_ARG, "expected (u64, u64)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // flush — (fd) -> [true]  (sync_all: data + metadata)
    // -------------------------------------------------------------------
    pub async fn fs_flush(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_flush"));
        };
        let [fd_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_flush"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoNumber::unapply(fd_par) {
            Some(fd) if fd >= 0 => {
                let r = self
                    .handles
                    .with_mut(fd as u64, |h| h.file.sync_all())
                    .await;
                match r {
                    None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                    Some(Err(e)) => err(io_err_code(&e), e.to_string()),
                    Some(Ok(())) => ok_bare(),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // stat — (canonPath) -> [true, record]  (symlink_metadata)
    // -------------------------------------------------------------------
    pub async fn fs_stat(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_stat"));
        };
        let [path_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_stat"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoString::unapply(path_par) {
            Some(path) => {
                let p = PathBuf::from(&path);
                match std::fs::symlink_metadata(&p) {
                    Ok(meta) => {
                        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or(&path);
                        ok_par(stat_record(name, &meta, self.mode))
                    }
                    Err(e) => err(io_err_code(&e), e.to_string()),
                }
            }
            None => err(FSERR_BAD_ARG, "expected String"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // exists — (canonPath) -> [true, Bool]
    // -------------------------------------------------------------------
    pub async fn fs_exists(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_exists"));
        };
        let [path_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_exists"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoString::unapply(path_par) {
            Some(path) => {
                let exists = std::fs::symlink_metadata(&path).is_ok();
                ok_bool(exists)
            }
            None => err(FSERR_BAD_ARG, "expected String"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // entries — (canonPath) -> [true, [record, ...]]  (sorted lex by name)
    // Per-entry stat error becomes a row with an `error` field.
    // -------------------------------------------------------------------
    pub async fn fs_entries(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_entries"));
        };
        let [path_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoString::unapply(path_par) {
            Some(path) => match std::fs::read_dir(&path) {
                Ok(rd) => {
                    let mut entries: Vec<(String, std::io::Result<std::fs::Metadata>)> = rd
                        .filter_map(|e| e.ok())
                        .map(|e| {
                            let name = e.file_name().to_string_lossy().into_owned();
                            let meta = std::fs::symlink_metadata(e.path());
                            (name, meta)
                        })
                        .collect();
                    entries.sort_by(|a, b| a.0.cmp(&b.0));
                    let rows: Vec<Par> = entries
                        .into_iter()
                        .map(|(name, meta_res)| match meta_res {
                            Ok(meta) => stat_record(&name, &meta, self.mode),
                            Err(e) => error_record(&name, &e.to_string()),
                        })
                        .collect();
                    ok_list(rows)
                }
                Err(e) => err(io_err_code(&e), e.to_string()),
            },
            None => err(FSERR_BAD_ARG, "expected String"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // rename — (fromCanon, toCanon) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_rename(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_rename"));
        };
        let [from_par, to_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_rename"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(from_par), RhoString::unapply(to_par)) {
            (Some(from), Some(to)) => match std::fs::rename(&from, &to) {
                Ok(()) => ok_bare(),
                Err(e) => {
                    // EXDEV (18) → cross-device link
                    let code = if e.raw_os_error() == Some(18) {
                        FSERR_CROSS_DEVICE
                    } else {
                        io_err_code(&e)
                    };
                    err(code, e.to_string())
                }
            },
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // copyFile — (fromCanon, toCanon) -> [true, nBytes]
    // -------------------------------------------------------------------
    pub async fn fs_copy_file(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_copy_file"));
        };
        let [from_par, to_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_copy_file"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(from_par), RhoString::unapply(to_par)) {
            (Some(from), Some(to)) => match std::fs::copy(&from, &to) {
                Ok(n) => ok_u64(n),
                Err(e) => err(io_err_code(&e), e.to_string()),
            },
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // removeFile — (canonPath) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_remove_file(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_remove_file"));
        };
        let [path_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_remove_file"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match RhoString::unapply(path_par) {
            Some(path) => match std::fs::remove_file(&path) {
                Ok(()) => ok_bare(),
                Err(e) => err(io_err_code(&e), e.to_string()),
            },
            None => err(FSERR_BAD_ARG, "expected String"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // removeDir — (canonPath, recursive: Bool) -> [true]
    // -------------------------------------------------------------------
    pub async fn fs_remove_dir(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        use super::super::rho_type::RhoBoolean;
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        let [path_par, recursive_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoString::unapply(path_par),
            RhoBoolean::unapply(recursive_par),
        ) {
            (Some(path), Some(recursive)) => {
                let r = if recursive {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_dir(&path)
                };
                match r {
                    Ok(()) => ok_bare(),
                    Err(e) => err(io_err_code(&e), e.to_string()),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected (String, Bool)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // chmod — (canonPath, modeBits) -> [true]
    // Bits pre-parsed by the agent layer; this handler takes u64 bits.
    // -------------------------------------------------------------------
    pub async fn fs_chmod(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_chmod"));
        };
        let [path_par, mode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_chmod"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(path_par), RhoNumber::unapply(mode_par)) {
            (Some(path), Some(bits)) if (0..=0o7777).contains(&bits) => {
                chmod_impl(&path, bits as u32)
            }
            _ => err(FSERR_BAD_ARG, "expected (String, u64) with bits <= 0o7777"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // chown — (canonPath, owner, group) -> [true]
    // Consensus-mode: returns FSERR_UNSUPPORTED.
    // Oracular: resolves names via NSS with the transient-vs-not-found split.
    // -------------------------------------------------------------------
    pub async fn fs_chown(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_chown"));
        };
        let [path_par, owner_par, group_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_chown"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = if self.mode == ConsensusMode::Consensus {
            err(FSERR_UNSUPPORTED, "chown unavailable in consensus mode")
        } else {
            match RhoString::unapply(path_par) {
                Some(path) => {
                    let owner_opt = maybe_string(owner_par);
                    let group_opt = maybe_string(group_par);
                    chown_impl(&path, owner_opt.as_deref(), group_opt.as_deref())
                }
                None => err(FSERR_BAD_ARG, "expected (String, String|Nil, String|Nil)"),
            }
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // quarantine — (rootCanon, rel) -> [true, canonPath]
    // Standalone canonicalize+escape-check.
    // -------------------------------------------------------------------
    pub async fn fs_quarantine(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_quarantine"));
        };
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_quarantine"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => match canonicalize_and_quarantine(Path::new(&root), &rel) {
                Ok(canon) => ok_string(canon.to_string_lossy().into_owned()),
                Err(e) => {
                    let (code, msg) = quarantine_err_reply(&e);
                    err(code, msg)
                }
            },
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // entriesStream — (canonPath) -> [true, streamFd]
    // Placeholder: returns FSERR_UNSUPPORTED until Phase 4 wires the
    // agent-side EntryStream that this native backs.
    // -------------------------------------------------------------------
    pub async fn fs_entries_stream(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_entries_stream"));
        };
        let [_path_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries_stream"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = err(FSERR_UNSUPPORTED, "entriesStream backing pending Phase 4");
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn maybe_string(p: &Par) -> Option<String> {
    // Treat both Nil (missing unforgeable/expr) and String as valid;
    // return None for Nil, Some(_) for String; anything else is caller
    // error handled by the arg-shape check.
    RhoString::unapply(p)
}

#[cfg(unix)]
fn chmod_impl(path: &str, bits: u32) -> Par {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(bits);
    match std::fs::set_permissions(path, perms) {
        Ok(()) => ok_bare(),
        Err(e) => err(io_err_code(&e), e.to_string()),
    }
}

#[cfg(not(unix))]
fn chmod_impl(_path: &str, _bits: u32) -> Par {
    err(FSERR_UNSUPPORTED, "chmod not supported on this platform")
}

#[cfg(unix)]
fn chown_impl(path: &str, owner: Option<&str>, group: Option<&str>) -> Par {
    use super::nss::{resolve_gid, resolve_uid};

    let uid = match owner {
        None => None,
        Some(name) => match resolve_uid(name) {
            Ok(Some(u)) => Some(u),
            Ok(None) => return err(FSERR_BAD_ARG, format!("unknown user {name}")),
            Err(e) => return err(FSERR_IO, e),
        },
    };
    let gid = match group {
        None => None,
        Some(name) => match resolve_gid(name) {
            Ok(Some(g)) => Some(g),
            Ok(None) => return err(FSERR_BAD_ARG, format!("unknown group {name}")),
            Err(e) => return err(FSERR_IO, e),
        },
    };

    use std::ffi::CString;
    let cpath = match CString::new(path) {
        Ok(c) => c,
        Err(e) => return err(FSERR_BAD_ARG, e.to_string()),
    };
    let rc = unsafe {
        libc::lchown(
            cpath.as_ptr(),
            uid.unwrap_or(u32::MAX),
            gid.unwrap_or(u32::MAX),
        )
    };
    if rc == 0 {
        ok_bare()
    } else {
        let e = std::io::Error::last_os_error();
        err(io_err_code(&e), e.to_string())
    }
}

#[cfg(not(unix))]
fn chown_impl(_path: &str, _owner: Option<&str>, _group: Option<&str>) -> Par {
    err(FSERR_UNSUPPORTED, "chown not supported on this platform")
}

// Silence unused-import warning when AccessMode is only used in tests/traits.
#[allow(dead_code)]
fn _use_access_mode(_a: AccessMode) {}
