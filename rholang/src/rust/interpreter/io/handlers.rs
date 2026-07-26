// The 22 native filesystem handlers.
//
// Each handler:
//   1. Unapplies the incoming contract call to extract
//      `(produce, is_replay, previous_output, args)`.
//   2. On replay, immediately re-sends `previous_output` — filesystem
//      calls are non-deterministic and must not be re-issued.  Consistent
//      with `gpt4`/`dalle3`/`ollama_chat`: no cost charged on the replay
//      branch (cost already accounted at capture time by the leader).
//   3. Otherwise validates arguments, quarantines any path via
//      `path::safe_descend`, dispatches to the syscall in a
//      `spawn_blocking` task (so long-blocking `fsync`/`copy` never
//      stalls the reactor), and builds the `[true, ...]` /
//      `[false, code, msg]` reply.
//
// Path safety: every path-taking handler takes `(rootCanon, rel)` and
// descends via `openat + O_NOFOLLOW` at each step.  The leaf operation
// is issued as an `*at` syscall against the resolved parent dirfd, so
// the resolution path used for the safety check is the exact same path
// used for the operation — TOCTOU-immune.
//
// Error messages are scrubbed via `io_msg_scrub` — we surface the
// `std::io::ErrorKind` classification but not the free-form message
// (which on some platforms includes the offending path, leaking the
// caller's root prefix).

use std::path::PathBuf;

use models::rhoapi::{ListParWithRandom, Par};
use tokio::task::spawn_blocking;

use super::super::contract_call::ContractCall;
use super::super::dispatch::RhoDispatch;
use super::super::errors::{illegal_argument_error, InterpreterError};
use super::super::rho_runtime::RhoISpace;
use super::super::rho_type::{RhoBoolean, RhoByteArray, RhoNumber, RhoString};
use super::errors::*;
use super::handle_table::{FileHandle, FileHandleTable};
use super::mode::{fopen_flags, parse_open_mode, AccessMode};
use super::path::{io_msg_scrub, quarantine_err_reply, safe_descend, safe_open, SafeParent};
use super::response::*;
use super::stat::{error_record, stat_record};
use super::ConsensusMode;

/// Cap on `fs_entries` output size — prevents a malicious caller pointing
/// the native at a million-entry directory and OOMing the node.
pub const MAX_ENTRIES: usize = 65_536;

/// Cap on `fs_write` payload — symmetric with `MAX_READ_BYTES`.
pub const MAX_WRITE_BYTES: u64 = 64 * 1024 * 1024;

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
            (Some(root), Some(rel), Some(mode_str)) => self.open_impl(root, rel, mode_str).await,
            _ => err(FSERR_BAD_ARG, "expected (String, String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    async fn open_impl(&self, root: String, rel: String, mode: String) -> Par {
        let intent = match parse_open_mode(&mode) {
            Some(i) => i,
            None => return err(FSERR_BAD_ARG, format!("unknown fopen mode {mode:?}")),
        };
        let root_pb = PathBuf::from(&root);
        let intent_copy = intent;
        // openat descent + safe_open in a blocking task — sync fs.
        let opened = spawn_blocking(move || {
            let (flags, mode_bits) = fopen_flags(intent_copy);
            super::path::safe_open(&root_pb, &rel, flags, mode_bits)
        })
        .await;
        let file = match opened {
            Err(join_err) => return err(FSERR_IO, join_err.to_string()),
            Ok(Err(qe)) => {
                let (code, msg) = quarantine_err_reply(&qe);
                return err(code, msg);
            }
            Ok(Ok(f)) => f,
        };
        // Reject non-regular files via fstat on the opened fd.  Because
        // we already have the fd (opened with O_NOFOLLOW), there's no
        // TOCTOU here.
        let meta = match file.metadata() {
            Ok(m) => m,
            Err(e) => return err(io_err_code(&e), io_msg_scrub(&e)),
        };
        if !meta.file_type().is_file() {
            return err(FSERR_UNSUPPORTED, "not a regular file");
        }
        let handle = FileHandle {
            file,
            canon_path: PathBuf::from(root).join(""),
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
        // We can't move a `&mut FileHandle` into spawn_blocking (it lives
        // behind an RwLock owned by `handles`).  Instead: take the fd's
        // raw fd, do the syscall on a blocking task, and let `File` be
        // reconstructed from the handle table on the next call.  We use
        // libc::pread directly so we don't need &mut File.
        let raw_fd = match self.handles.raw_fd(fd).await {
            Some(rfd) => rfd,
            None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
        };
        let result = spawn_blocking(move || {
            let mut buf = vec![0u8; n as usize];
            let got = unsafe {
                if let Some(off) = offset {
                    libc::pread(raw_fd, buf.as_mut_ptr() as *mut _, n as usize, off as i64)
                } else {
                    libc::read(raw_fd, buf.as_mut_ptr() as *mut _, n as usize)
                }
            };
            if got < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                buf.truncate(got as usize);
                Ok(buf)
            }
        })
        .await;
        match result {
            Err(join_err) => err(FSERR_IO, join_err.to_string()),
            Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
            Ok(Ok(bytes)) => ok_bytes(bytes),
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
            (Some(fd), Some(bytes)) if fd >= 0 => self.write_impl(fd as u64, bytes, None).await,
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
                self.write_impl(fd as u64, bytes, Some(off as u64)).await
            }
            _ => err(FSERR_BAD_ARG, "expected (u64, u64, ByteArray)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    async fn write_impl(&self, fd: u64, bytes: Vec<u8>, offset: Option<u64>) -> Par {
        if bytes.len() as u64 > MAX_WRITE_BYTES {
            return err(
                FSERR_QUOTA_EXCEEDED,
                format!("write {} exceeds MAX_WRITE_BYTES", bytes.len()),
            );
        }
        let raw_fd = match self.handles.raw_fd(fd).await {
            Some(rfd) => rfd,
            None => return err(FSERR_CLOSED, format!("unknown fd {fd}")),
        };
        let result = spawn_blocking(move || {
            let n = unsafe {
                if let Some(off) = offset {
                    libc::pwrite(raw_fd, bytes.as_ptr() as *const _, bytes.len(), off as i64)
                } else {
                    libc::write(raw_fd, bytes.as_ptr() as *const _, bytes.len())
                }
            };
            if n < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(n as u64)
            }
        })
        .await;
        match result {
            Err(join_err) => err(FSERR_IO, join_err.to_string()),
            Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
            Ok(Ok(n)) => ok_u64(n),
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
                let whence_code = match w.as_str() {
                    "set" if off >= 0 => Some(libc::SEEK_SET),
                    "cur" => Some(libc::SEEK_CUR),
                    "end" => Some(libc::SEEK_END),
                    _ => None,
                };
                match whence_code {
                    None => err(FSERR_BAD_ARG, "expected whence in {set,cur,end}"),
                    Some(whence) => match self.handles.raw_fd(fd as u64).await {
                        None => err(FSERR_CLOSED, format!("unknown fd {fd}")),
                        Some(raw_fd) => {
                            let r = spawn_blocking(move || unsafe {
                                let pos = libc::lseek(raw_fd, off, whence);
                                if pos < 0 {
                                    Err(std::io::Error::last_os_error())
                                } else {
                                    Ok(pos as u64)
                                }
                            })
                            .await;
                            match r {
                                Err(je) => err(FSERR_IO, je.to_string()),
                                Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                                Ok(Ok(pos)) => ok_u64(pos),
                            }
                        }
                    },
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
                let raw_fd = match self.handles.raw_fd(fd as u64).await {
                    Some(r) => r,
                    None => {
                        let out = vec![err(FSERR_CLOSED, format!("unknown fd {fd}"))];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let r = spawn_blocking(move || unsafe {
                    let pos = libc::lseek(raw_fd, 0, libc::SEEK_CUR);
                    if pos < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(pos as u64)
                    }
                })
                .await;
                match r {
                    Err(je) => err(FSERR_IO, je.to_string()),
                    Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                    Ok(Ok(pos)) => ok_u64(pos),
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
                let raw_fd = match self.handles.raw_fd(fd as u64).await {
                    Some(r) => r,
                    None => {
                        let out = vec![err(FSERR_CLOSED, format!("unknown fd {fd}"))];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let r = spawn_blocking(move || unsafe {
                    let mut sb: libc::stat = std::mem::zeroed();
                    if libc::fstat(raw_fd, &mut sb) < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(sb.st_size as u64)
                    }
                })
                .await;
                match r {
                    Err(je) => err(FSERR_IO, je.to_string()),
                    Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                    Ok(Ok(n)) => ok_u64(n),
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
                    let raw_fd = match self.handles.raw_fd(fd as u64).await {
                        Some(r) => r,
                        None => {
                            let out = vec![err(FSERR_CLOSED, format!("unknown fd {fd}"))];
                            produce(&out, ack).await?;
                            return Ok(out);
                        }
                    };
                    let r = spawn_blocking(move || unsafe {
                        if libc::ftruncate(raw_fd, n) < 0 {
                            Err(std::io::Error::last_os_error())
                        } else {
                            Ok(())
                        }
                    })
                    .await;
                    match r {
                        Err(je) => err(FSERR_IO, je.to_string()),
                        Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                        Ok(Ok(())) => ok_bare(),
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
    // flush — (fd) -> [true]  (fsync: data + metadata)
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
                let raw_fd = match self.handles.raw_fd(fd as u64).await {
                    Some(r) => r,
                    None => {
                        let out = vec![err(FSERR_CLOSED, format!("unknown fd {fd}"))];
                        produce(&out, ack).await?;
                        return Ok(out);
                    }
                };
                let r = spawn_blocking(move || unsafe {
                    if libc::fsync(raw_fd) < 0 {
                        Err(std::io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                })
                .await;
                match r {
                    Err(je) => err(FSERR_IO, je.to_string()),
                    Ok(Err(e)) => err(io_err_code(&e), io_msg_scrub(&e)),
                    Ok(Ok(())) => ok_bare(),
                }
            }
            _ => err(FSERR_BAD_ARG, "expected u64"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // stat — (rootCanon, rel) -> [true, record]
    // Uses fstatat(AT_SYMLINK_NOFOLLOW) via safe descent.
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
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_stat"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let mode = self.mode;
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let leaf_name = leaf_of(&rel);
                let root_pb = PathBuf::from(root);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend(&root_pb, &rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (code, msg) = quarantine_err_reply(&qe);
                            return err(code, msg);
                        }
                    };
                    match fstatat_meta(&parent) {
                        Ok(m) => ok_par(stat_record(&leaf_name, &m, mode)),
                        Err(e) => err(io_err_code(&e), io_msg_scrub(&e)),
                    }
                })
                .await
                .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // exists — (rootCanon, rel) -> [true, Bool]
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
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_exists"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(root);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend(&root_pb, &rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            // Not-found or symlink → exists is false.  A
                            // quarantine failure (escape/absolute/etc.)
                            // is a caller error, surface as bad arg.
                            use super::path::QuarantineError::*;
                            return match qe {
                                EscapesRoot | SymlinkComponent => {
                                    let (c, m) = quarantine_err_reply(&qe);
                                    err(c, m)
                                }
                                Empty | RootSelf => {
                                    let (c, m) = quarantine_err_reply(&qe);
                                    err(c, m)
                                }
                                IoError(_) => ok_bool(false),
                            };
                        }
                    };
                    let ok = fstatat_meta(&parent).is_ok();
                    ok_bool(ok)
                })
                .await
                .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // entries — (rootCanon, rel) -> [true, [record, ...]]
    // Sorted lex by name; capped at MAX_ENTRIES; per-entry stat error
    // becomes a row with an `error` field.
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
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let mode = self.mode;
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(root);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend(&root_pb, &rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (code, msg) = quarantine_err_reply(&qe);
                            return err(code, msg);
                        }
                    };
                    // Open the target directory (safely, via openat +
                    // O_NOFOLLOW|O_DIRECTORY off the parent dirfd).
                    let dir_fd = unsafe {
                        libc::openat(
                            parent.as_raw_fd(),
                            parent.leaf_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        )
                    };
                    if dir_fd < 0 {
                        let e = std::io::Error::last_os_error();
                        return err(io_err_code(&e), io_msg_scrub(&e));
                    }
                    // Dup for the readdir stream so we can also use
                    // dir_fd for openat per entry.
                    let read_fd = unsafe { libc::dup(dir_fd) };
                    if read_fd < 0 {
                        let e = std::io::Error::last_os_error();
                        unsafe { libc::close(dir_fd) };
                        return err(io_err_code(&e), io_msg_scrub(&e));
                    }
                    let entries = read_dir_capped(read_fd, MAX_ENTRIES);
                    match entries {
                        Err(e) => {
                            unsafe { libc::close(dir_fd) };
                            err(io_err_code(&e), io_msg_scrub(&e))
                        }
                        Ok((mut names, hit_cap)) => {
                            if hit_cap {
                                unsafe { libc::close(dir_fd) };
                                return err(
                                    FSERR_QUOTA_EXCEEDED,
                                    format!(
                                        "entries exceeds MAX_ENTRIES={MAX_ENTRIES}; use \
                                         entriesStream for large directories",
                                    ),
                                );
                            }
                            names.sort();
                            let rows: Vec<Par> = names
                                .into_iter()
                                .map(|name| entry_stat_row(dir_fd, &name, mode))
                                .collect();
                            unsafe { libc::close(dir_fd) };
                            ok_list(rows)
                        }
                    }
                })
                .await
                .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // rename — (fromRootCanon, fromRel, toRootCanon, toRel) -> [true]
    // Uses renameat between two safely-descended parents.
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
        let [from_root_par, from_rel_par, to_root_par, to_rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_rename"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoString::unapply(from_root_par),
            RhoString::unapply(from_rel_par),
            RhoString::unapply(to_root_par),
            RhoString::unapply(to_rel_par),
        ) {
            (Some(from_root), Some(from_rel), Some(to_root), Some(to_rel)) => {
                let from_root_pb = PathBuf::from(from_root);
                let to_root_pb = PathBuf::from(to_root);
                spawn_blocking(move || -> Par {
                    let from_parent = match safe_descend(&from_root_pb, &from_rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let to_parent = match safe_descend(&to_root_pb, &to_rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let rc = unsafe {
                        libc::renameat(
                            from_parent.as_raw_fd(),
                            from_parent.leaf_ptr(),
                            to_parent.as_raw_fd(),
                            to_parent.leaf_ptr(),
                        )
                    };
                    if rc == 0 {
                        ok_bare()
                    } else {
                        let e = std::io::Error::last_os_error();
                        let code = if e.raw_os_error() == Some(libc::EXDEV) {
                            FSERR_CROSS_DEVICE
                        } else {
                            io_err_code(&e)
                        };
                        err(code, io_msg_scrub(&e))
                    }
                })
                .await
                .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
            }
            _ => err(FSERR_BAD_ARG, "expected 4 String args"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // copyFile — (fromRootCanon, fromRel, toRootCanon, toRel) -> [true, nBytes]
    // Uses safe_open on both sides + std::io::copy on File objects.
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
        let [from_root_par, from_rel_par, to_root_par, to_rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_copy_file"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoString::unapply(from_root_par),
            RhoString::unapply(from_rel_par),
            RhoString::unapply(to_root_par),
            RhoString::unapply(to_rel_par),
        ) {
            (Some(from_root), Some(from_rel), Some(to_root), Some(to_rel)) => {
                let from_pb = PathBuf::from(from_root);
                let to_pb = PathBuf::from(to_root);
                spawn_blocking(move || -> Par {
                    let mut src = match safe_open(&from_pb, &from_rel, libc::O_RDONLY, 0) {
                        Ok(f) => f,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let mut dst = match safe_open(
                        &to_pb,
                        &to_rel,
                        libc::O_WRONLY | libc::O_CREAT | libc::O_TRUNC,
                        0o644,
                    ) {
                        Ok(f) => f,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    match std::io::copy(&mut src, &mut dst) {
                        Ok(n) => ok_u64(n),
                        Err(e) => err(io_err_code(&e), io_msg_scrub(&e)),
                    }
                })
                .await
                .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
            }
            _ => err(FSERR_BAD_ARG, "expected 4 String args"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // removeFile — (rootCanon, rel) -> [true]
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
        let [root_par, rel_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_remove_file"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(root);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend(&root_pb, &rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let rc = unsafe { libc::unlinkat(parent.as_raw_fd(), parent.leaf_ptr(), 0) };
                    if rc == 0 {
                        ok_bare()
                    } else {
                        let e = std::io::Error::last_os_error();
                        err(io_err_code(&e), io_msg_scrub(&e))
                    }
                })
                .await
                .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // removeDir — (rootCanon, rel, recursive: Bool) -> [true]
    // Non-recursive: unlinkat(AT_REMOVEDIR).
    // Recursive: descend into the target and unlinkat every entry (safe).
    // -------------------------------------------------------------------
    pub async fn fs_remove_dir(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Result<Vec<Par>, InterpreterError> {
        let Some((produce, is_replay, previous, args)) =
            self.is_contract_call().unapply(contract_args)
        else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        let [root_par, rel_par, recursive_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_remove_dir"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoBoolean::unapply(recursive_par),
        ) {
            (Some(root), Some(rel), Some(recursive)) => {
                let root_pb = PathBuf::from(root);
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend(&root_pb, &rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    if recursive {
                        if let Err(e) = remove_dir_recursive(parent.as_raw_fd(), parent.leaf_ptr())
                        {
                            return err(io_err_code(&e), io_msg_scrub(&e));
                        }
                        ok_bare()
                    } else {
                        let rc = unsafe {
                            libc::unlinkat(
                                parent.as_raw_fd(),
                                parent.leaf_ptr(),
                                libc::AT_REMOVEDIR,
                            )
                        };
                        if rc == 0 {
                            ok_bare()
                        } else {
                            let e = std::io::Error::last_os_error();
                            err(io_err_code(&e), io_msg_scrub(&e))
                        }
                    }
                })
                .await
                .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String, Bool)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // chmod — (rootCanon, rel, modeBits) -> [true]
    // fchmodat(AT_SYMLINK_NOFOLLOW) — spec-mandated symlink safety.
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
        let [root_par, rel_par, mode_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_chmod"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = match (
            RhoString::unapply(root_par),
            RhoString::unapply(rel_par),
            RhoNumber::unapply(mode_par),
        ) {
            (Some(root), Some(rel), Some(bits)) if (0..=0o7777).contains(&bits) => {
                let root_pb = PathBuf::from(root);
                let bits = bits as libc::mode_t;
                spawn_blocking(move || -> Par {
                    let parent = match safe_descend(&root_pb, &rel) {
                        Ok(p) => p,
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            return err(c, m);
                        }
                    };
                    let rc = unsafe {
                        libc::fchmodat(
                            parent.as_raw_fd(),
                            parent.leaf_ptr(),
                            bits,
                            libc::AT_SYMLINK_NOFOLLOW,
                        )
                    };
                    if rc == 0 {
                        ok_bare()
                    } else {
                        let e = std::io::Error::last_os_error();
                        // ENOTSUP means the platform doesn't honor
                        // AT_SYMLINK_NOFOLLOW on chmod (Linux, some
                        // filesystems).  In that case there's no
                        // symlink-safe chmod primitive; report
                        // UNSUPPORTED so the caller sees the failure
                        // rather than silently following.
                        let code = if e.raw_os_error() == Some(libc::ENOTSUP)
                            || e.raw_os_error() == Some(libc::EOPNOTSUPP)
                        {
                            FSERR_UNSUPPORTED
                        } else {
                            io_err_code(&e)
                        };
                        err(code, io_msg_scrub(&e))
                    }
                })
                .await
                .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String, u64<=0o7777)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // chown — (rootCanon, rel, owner, group) -> [true]
    // Consensus mode: returns FSERR_UNSUPPORTED.
    // Oracular: fchownat(AT_SYMLINK_NOFOLLOW).
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
        let [root_par, rel_par, owner_par, group_par, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_chown"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = if self.mode == ConsensusMode::Consensus {
            err(FSERR_UNSUPPORTED, "chown unavailable in consensus mode")
        } else {
            match (RhoString::unapply(root_par), RhoString::unapply(rel_par)) {
                (Some(root), Some(rel)) => {
                    let owner_opt = RhoString::unapply(owner_par);
                    let group_opt = RhoString::unapply(group_par);
                    let root_pb = PathBuf::from(root);
                    chown_impl(&root_pb, rel, owner_opt, group_opt).await
                }
                _ => err(
                    FSERR_BAD_ARG,
                    "expected (String, String, String|Nil, String|Nil)",
                ),
            }
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // quarantine — (rootCanon, rel) -> [true, canonPath]
    // Standalone safety check that also returns a diagnostic display path
    // (procfs magic-link resolution of the parent dirfd + leaf).  Note:
    // the returned canonPath echoes back the (already caller-known)
    // resolved path; other handlers do NOT accept caller-supplied
    // canonPaths.
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
            (Some(root), Some(rel)) => {
                let root_pb = PathBuf::from(&root);
                spawn_blocking(move || -> Par {
                    match safe_descend(&root_pb, &rel) {
                        Ok(_) => {
                            // Return the caller-supplied joined path;
                            // safe_descend already verified it doesn't
                            // escape.  This is deterministic (no
                            // canonicalize call, so no host drift).
                            ok_string(root_pb.join(&rel).to_string_lossy().into_owned())
                        }
                        Err(qe) => {
                            let (c, m) = quarantine_err_reply(&qe);
                            err(c, m)
                        }
                    }
                })
                .await
                .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
            }
            _ => err(FSERR_BAD_ARG, "expected (String, String)"),
        };
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }

    // -------------------------------------------------------------------
    // entriesStream — (rootCanon, rel) -> [true, streamFd]
    // Placeholder: returns FSERR_UNSUPPORTED.  The backing streaming
    // primitive (a per-runtime dir-handle table analogous to
    // FileHandleTable, with `next(fd)` / `close(fd)` operators) is
    // scoped for Phase 1 tail-end but not yet implemented; Phase 4
    // wires the agent-side EntryStream on top of it.
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
        let [_root, _rel, ack] = args.as_slice() else {
            return Err(illegal_argument_error("fs_entries_stream"));
        };
        if is_replay {
            produce(&previous, ack).await?;
            return Ok(previous);
        }
        let reply = err(
            FSERR_UNSUPPORTED,
            "entriesStream backing not yet implemented (Phase 1 tail-end)",
        );
        let out = vec![reply];
        produce(&out, ack).await?;
        Ok(out)
    }
}

// ---------------------------------------------------------------------
// Helpers — pure fns (no self) called from spawn_blocking closures.
// ---------------------------------------------------------------------

fn leaf_of(rel: &str) -> String {
    std::path::Path::new(rel)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.to_string())
}

/// Fetch a `std::fs::Metadata` for the leaf named by `parent`.  Opens
/// the leaf via openat + O_NOFOLLOW off the parent dirfd, then reads
/// metadata.  A symlink leaf yields `ELOOP` — the caller decides how to
/// surface that.
fn fstatat_meta(parent: &SafeParent) -> std::io::Result<std::fs::Metadata> {
    use std::os::fd::FromRawFd;
    unsafe {
        let fd = libc::openat(
            parent.as_raw_fd(),
            parent.leaf_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let file = std::fs::File::from_raw_fd(fd);
        file.metadata()
    }
}

/// Build a stat/error record for one entry inside `dir_fd`.  Opens the
/// entry via openat + O_NOFOLLOW; regular/directory entries produce a
/// full `stat_record`, symlinks and unreadable entries produce an
/// `error_record` (spec §Dir.entries: per-entry error becomes a row).
fn entry_stat_row(dir_fd: libc::c_int, name: &std::ffi::OsStr, mode: ConsensusMode) -> Par {
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    let display = name.to_string_lossy().into_owned();
    let cname = match std::ffi::CString::new(name.as_bytes()) {
        Ok(c) => c,
        Err(_) => return error_record(&display, "invalid filename"),
    };
    unsafe {
        let fd = libc::openat(
            dir_fd,
            cname.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if fd < 0 {
            let e = std::io::Error::last_os_error();
            return error_record(&display, &io_msg_scrub(&e));
        }
        let file = std::fs::File::from_raw_fd(fd);
        match file.metadata() {
            Ok(m) => stat_record(&display, &m, mode),
            Err(e) => error_record(&display, &io_msg_scrub(&e)),
        }
    }
}

#[allow(clippy::too_many_lines)]
fn read_dir_capped(
    dir_fd: libc::c_int,
    max: usize,
) -> std::io::Result<(Vec<std::ffi::OsString>, bool)> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    unsafe {
        let dir = libc::fdopendir(dir_fd);
        if dir.is_null() {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            return Err(e);
        }
        let mut names: Vec<OsString> = Vec::new();
        let mut hit_cap = false;
        loop {
            // Reset errno; readdir returns NULL on both EOF and error.
            errno_reset();
            let ent = libc::readdir(dir);
            if ent.is_null() {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(0) {
                    break; // Clean EOF.
                }
                libc::closedir(dir);
                return Err(e);
            }
            let name_ptr = (*ent).d_name.as_ptr();
            let name_c = std::ffi::CStr::from_ptr(name_ptr);
            let name_bytes = name_c.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            if names.len() >= max {
                hit_cap = true;
                break;
            }
            names.push(OsString::from_vec(name_bytes.to_vec()));
        }
        libc::closedir(dir);
        Ok((names, hit_cap))
    }
}

/// Recursive symlink-safe rmdir.  Descends from `parent` into `leaf`
/// (must be a directory; ELOOP if symlink), unlinks every entry, then
/// removes the directory itself.
fn remove_dir_recursive(parent_fd: libc::c_int, leaf: *const libc::c_char) -> std::io::Result<()> {
    unsafe {
        let dir_fd = libc::openat(
            parent_fd,
            leaf,
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        );
        if dir_fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // Dup dir_fd so we can readdir on one copy and use the other for
        // unlinkat.
        let dup_fd = libc::dup(dir_fd);
        if dup_fd < 0 {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            return Err(e);
        }
        let dir = libc::fdopendir(dup_fd);
        if dir.is_null() {
            let e = std::io::Error::last_os_error();
            libc::close(dir_fd);
            libc::close(dup_fd);
            return Err(e);
        }
        loop {
            errno_reset();
            let ent = libc::readdir(dir);
            if ent.is_null() {
                let e = std::io::Error::last_os_error();
                if e.raw_os_error() == Some(0) {
                    break;
                }
                libc::closedir(dir);
                libc::close(dir_fd);
                return Err(e);
            }
            let name_ptr = (*ent).d_name.as_ptr();
            let name_c = std::ffi::CStr::from_ptr(name_ptr);
            let name_bytes = name_c.to_bytes();
            if name_bytes == b"." || name_bytes == b".." {
                continue;
            }
            // Try file first; if it's a directory, recurse.
            let file_rc = libc::unlinkat(dir_fd, name_ptr, 0);
            if file_rc == 0 {
                continue;
            }
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::EISDIR) || e.raw_os_error() == Some(libc::EPERM) {
                if let Err(inner) = remove_dir_recursive(dir_fd, name_ptr) {
                    libc::closedir(dir);
                    libc::close(dir_fd);
                    return Err(inner);
                }
                continue;
            }
            libc::closedir(dir);
            libc::close(dir_fd);
            return Err(e);
        }
        libc::closedir(dir);
        libc::close(dir_fd);
        // Finally remove the directory itself.
        if libc::unlinkat(parent_fd, leaf, libc::AT_REMOVEDIR) < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}

async fn chown_impl(
    root: &std::path::Path,
    rel: String,
    owner: Option<String>,
    group: Option<String>,
) -> Par {
    use super::nss::{resolve_gid, resolve_uid};

    let uid = match owner {
        None => u32::MAX, // libc: -1 means "no change"
        Some(name) => match resolve_uid(&name) {
            Ok(Some(u)) => u,
            Ok(None) => return err(FSERR_BAD_ARG, format!("unknown user {name}")),
            Err(e) => return err(FSERR_IO, e),
        },
    };
    let gid = match group {
        None => u32::MAX,
        Some(name) => match resolve_gid(&name) {
            Ok(Some(g)) => g,
            Ok(None) => return err(FSERR_BAD_ARG, format!("unknown group {name}")),
            Err(e) => return err(FSERR_IO, e),
        },
    };
    let root_pb = root.to_path_buf();
    spawn_blocking(move || -> Par {
        let parent = match safe_descend(&root_pb, &rel) {
            Ok(p) => p,
            Err(qe) => {
                let (c, m) = quarantine_err_reply(&qe);
                return err(c, m);
            }
        };
        let rc = unsafe {
            libc::fchownat(
                parent.as_raw_fd(),
                parent.leaf_ptr(),
                uid,
                gid,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc == 0 {
            ok_bare()
        } else {
            let e = std::io::Error::last_os_error();
            err(io_err_code(&e), io_msg_scrub(&e))
        }
    })
    .await
    .unwrap_or_else(|je| err(FSERR_IO, je.to_string()))
}

/// Silence unused-import warning on AccessMode (used in Phase 5 by the
/// File-agent wiring).
#[allow(dead_code)]
fn _use_access_mode(_a: AccessMode) {}

/// Portable errno reset.  `readdir` returns NULL on both EOF and error;
/// distinguishing them requires clearing errno beforehand and checking
/// it after.  errno lives at platform-specific TLS addresses.
#[cfg(target_os = "macos")]
unsafe fn errno_reset() { *libc::__error() = 0; }

#[cfg(target_os = "linux")]
unsafe fn errno_reset() { *libc::__errno_location() = 0; }

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
unsafe fn errno_reset() {
    compile_error!("Unsupported platform for File I/O FIP native primitives");
}
