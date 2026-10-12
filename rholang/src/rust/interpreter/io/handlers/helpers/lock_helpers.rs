// Lock-family primitives.  Four helpers shared across the four
// Lock-family handlers (fs_lock_range, fs_lock_sequential,
// fs_release_lock, fs_unlock_range — the last three yet to land):
//
//   - `resolve_lock_mode(par) -> Option<LockMode>` — parse the
//     Rholang-supplied mode string into the registry's typed
//     mode (`"r"` → Read, `"w"` → Write).  Fail-closed on any
//     other shape (mirrors `resolve_cmode`'s discipline).
//   - `holder_id_of(par) -> HolderId` — derive a stable 32-byte
//     holder id from an opaque Rholang Par (per convention: the
//     caller-cap's per-instance `this` GPrivate name).  Uses the
//     same Blake2b256 provider as `ack_channel_hash` so holder
//     identity is deterministic across validators.
//   - `dev_inode_from_fd_via_table(handles, fd) -> Result<(u64, u64),
//     FSERR>` — resolve an fd through the handle table to its
//     `(dev, inode)` pair.  Reads the handle's `file: Arc<File>`
//     and runs `fstat`.  Shadow handles (no real file) surface
//     `FSERR_CLOSED`.
//   - `lock_err_reply(le) -> Par` — map a `LockError` from the
//     registry into the corresponding FSERR reply shape.
//
// # SAFETY contracts
//
// `dev_inode_from_fd_via_table` holds the fd open through an
// `Arc<File>` for the duration of the `fstat` syscall.  See
// inline SAFETY comment on the unsafe `libc::fstat` block.

use models::rhoapi::Par;

use crate::rust::interpreter::io::errors::{
    FserrCode, FSERR_BAD_ARG, FSERR_BUSY, FSERR_CANCELLED, FSERR_CLOSED, FSERR_DEADLOCK, FSERR_IO,
    FSERR_QUOTA_EXCEEDED,
};
use crate::rust::interpreter::io::handle_table::FileHandleTable;
use crate::rust::interpreter::io::lock::{HolderId, LockError, LockMode};
use crate::rust::interpreter::io::path::io_msg_scrub;
use crate::rust::interpreter::io::response::err;
use crate::rust::interpreter::rho_type::RhoString;

/// Parse the Rholang-supplied lock-mode string.  Fail-closed on
/// any shape other than `"r"` / `"w"` — matches `resolve_cmode`'s
/// discipline.
pub fn resolve_lock_mode(par: &Par) -> Option<LockMode> {
    match RhoString::unapply(par).as_deref() {
        Some("r") => Some(LockMode::Read),
        Some("w") => Some(LockMode::Write),
        _ => None,
    }
}

/// Derive a stable 32-byte `HolderId` from an opaque Rholang Par.
/// Uses the same Blake2b256 provider as `ack_channel_hash`, so
/// equal-Par callers hash to the same bytes deterministically.
///
/// Panics (release-mode `assert_eq!`) if the digest length is
/// not 32 bytes — same fail-hard discipline as `ack_channel_hash`.
pub fn holder_id_of(par: &Par) -> HolderId {
    let h = rspace_plus_plus::rspace::hashing::stable_hash_provider::hash(par).bytes();
    assert_eq!(
        h.len(),
        32,
        "Blake2b256 must produce 32-byte digest; got {} — HolderId hard-depends on \
         a fixed 32-byte hash width",
        h.len()
    );
    let mut out = [0u8; 32];
    out.copy_from_slice(&h);
    HolderId::from_bytes(out)
}

/// Resolve an fd via the handle table + `fstat` to its
/// `(dev, inode)` pair.  Used by lock handlers to key the
/// LockRegistry's per-file state on real on-disk identity (so
/// different fds on the same file see the same lock state).
///
/// Returns:
///   - `Ok((dev, inode))` on success.
///   - `Err((FSERR_CLOSED, "fd unknown or shadow handle"))` if
///     the fd is absent OR a Phase-2 shadow with no real file.
///   - `Err((FSERR_IO, scrubbed_msg))` on `fstat` failure.
pub async fn dev_inode_from_fd_via_table(
    handles: &FileHandleTable,
    fd: u64,
) -> Result<(u64, u64), (FserrCode, String)> {
    let Some(file_arc) = handles.raw_fd(fd).await else {
        return Err((FSERR_CLOSED, "fd unknown or shadow handle".to_string()));
    };
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        let raw = file_arc.as_raw_fd();
        // SAFETY: `file_arc` (an `Arc<File>`) keeps the
        // underlying fd open for the lifetime of this scope,
        // so `raw` is a valid open fd.  `libc::stat` is POD —
        // zeroed init is a valid bit pattern.  `fstat` writes
        // into `st` and doesn't retain either pointer past
        // return.
        unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            if libc::fstat(raw, &mut st) < 0 {
                let e = std::io::Error::last_os_error();
                return Err((FSERR_IO, io_msg_scrub(&e)));
            }
            #[allow(clippy::unnecessary_cast)]
            Ok((st.st_dev as u64, st.st_ino as u64))
        }
    }
    #[cfg(not(unix))]
    {
        let _ = file_arc;
        Ok((0, 0))
    }
}

/// Map a `LockError` into the corresponding FSERR reply shape.
/// Caller wraps via `HandlerReply::Err(lock_err_reply(le))`.
pub fn lock_err_reply(le: LockError) -> Par {
    match le {
        LockError::Busy => err(FSERR_BUSY, "range lock unavailable"),
        LockError::Closed => err(FSERR_CLOSED, "lock id not held"),
        LockError::BadArg => err(FSERR_BAD_ARG, "invalid lock argument"),
        LockError::QuotaExceeded => err(
            FSERR_QUOTA_EXCEEDED,
            "range-lock cap exceeded for this file",
        ),
        LockError::Cancelled => err(FSERR_CANCELLED, "wait:true lock acquisition cancelled"),
        LockError::Deadlock => err(
            FSERR_DEADLOCK,
            "wait:true lock acquisition would close cross-deploy wait-for cycle",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::rho_type::{RhoBoolean, RhoNumber};

    /// `resolve_lock_mode` accepts the two canonical lock-mode
    /// strings and only those.
    #[test]
    fn resolve_lock_mode_accepts_r_and_w() {
        assert_eq!(
            resolve_lock_mode(&RhoString::create_par("r".to_string())),
            Some(LockMode::Read),
        );
        assert_eq!(
            resolve_lock_mode(&RhoString::create_par("w".to_string())),
            Some(LockMode::Write),
        );
    }

    /// LOAD-BEARING: `resolve_lock_mode` fail-closes on any other
    /// shape (case variants, non-string Pars, empty string).
    /// Mirrors `resolve_cmode`'s discipline — a bogus mode must
    /// NOT silently default to a weaker lock.
    #[test]
    fn resolve_lock_mode_fails_closed_on_bogus_shapes() {
        assert!(resolve_lock_mode(&RhoString::create_par("R".to_string())).is_none());
        assert!(resolve_lock_mode(&RhoString::create_par("W".to_string())).is_none());
        assert!(resolve_lock_mode(&RhoString::create_par("read".to_string())).is_none());
        assert!(resolve_lock_mode(&RhoString::create_par("".to_string())).is_none());
        assert!(resolve_lock_mode(&RhoNumber::create_par(42)).is_none());
        assert!(resolve_lock_mode(&RhoBoolean::create_par(true)).is_none());
    }

    /// `holder_id_of` is deterministic across calls on the same
    /// Par.  Essential for leader + follower to compute matching
    /// HolderIds from the same Rholang caller-cap name.
    #[test]
    fn holder_id_of_is_deterministic() {
        let p = RhoString::create_par("holder-par".to_string());
        assert_eq!(holder_id_of(&p), holder_id_of(&p));
    }

    /// Distinct Pars hash to distinct HolderIds — a regression
    /// that returned a constant HolderId would collapse the
    /// LockRegistry's holder-identity invariant.
    #[test]
    fn holder_id_of_distinguishes_distinct_pars() {
        let a = RhoString::create_par("holder-a".to_string());
        let b = RhoString::create_par("holder-b".to_string());
        assert_ne!(holder_id_of(&a), holder_id_of(&b));
    }

    /// `lock_err_reply` maps each `LockError` variant to a
    /// non-default Par.  Exhaustive variant coverage pins the
    /// handler's err-reply shape against a future LockError
    /// variant addition that forgot to update this function.
    #[test]
    fn lock_err_reply_covers_every_variant() {
        for le in [
            LockError::Busy,
            LockError::Closed,
            LockError::BadArg,
            LockError::QuotaExceeded,
            LockError::Cancelled,
            LockError::Deadlock,
        ] {
            let reply = lock_err_reply(le);
            assert_ne!(
                prost::Message::encode_to_vec(&reply),
                prost::Message::encode_to_vec(&Par::default()),
                "lock_err_reply({le:?}) produced default/empty Par",
            );
        }
    }

    /// Runtime behavior of `dev_inode_from_fd_via_table` requires
    /// a FileHandleTable + real fd fixture.  Deferred to
    /// integration tests — the pre-trait fileio tests cover the
    /// behavior.
    #[tokio::test]
    async fn dev_inode_from_fd_unknown_fd_returns_fserr_closed() {
        let handles = FileHandleTable::new();
        let result = dev_inode_from_fd_via_table(&handles, 0xdeadbeef).await;
        match result {
            Err((code, _)) => assert_eq!(code, FSERR_CLOSED),
            Ok(_) => panic!("expected FSERR_CLOSED for unknown fd"),
        }
    }
}
