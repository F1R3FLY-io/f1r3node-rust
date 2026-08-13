// String error codes returned to Rholang callers as the second element
// of `[false, code, msg]` responses.  Every code is spec-canonical
// (§Errors).

pub const FSERR_BAD_ARG: &str = "FSERR_BAD_ARG";
pub const FSERR_IO: &str = "FSERR_IO";
pub const FSERR_NOT_FOUND: &str = "FSERR_NOT_FOUND";
pub const FSERR_ALREADY_EXISTS: &str = "FSERR_ALREADY_EXISTS";
pub const FSERR_PERM: &str = "FSERR_PERM";
pub const FSERR_UNSUPPORTED: &str = "FSERR_UNSUPPORTED";
pub const FSERR_QUARANTINE: &str = "FSERR_QUARANTINE";
pub const FSERR_CLOSED: &str = "FSERR_CLOSED";
pub const FSERR_BUSY: &str = "FSERR_BUSY";
pub const FSERR_QUOTA_EXCEEDED: &str = "FSERR_QUOTA_EXCEEDED";
pub const FSERR_CROSS_DEVICE: &str = "FSERR_CROSS_DEVICE";
/// Slice-8b `wait: true` acquisition cancelled — either explicitly
/// via `LockRegistry::cancel_wait`, or by the deploy-end sweep in
/// `WalDeployScope::drop` when a deploy ends with waiters still
/// parked.  Distinct from `FSERR_BUSY` so callers can tell "conflict
/// at request time" apart from "was in the queue but got cancelled".
pub const FSERR_CANCELLED: &str = "FSERR_CANCELLED";

use std::io;

/// Map a `std::io::Error` kind to a stable FSERR code.
pub fn io_err_code(e: &io::Error) -> &'static str {
    use io::ErrorKind::*;
    match e.kind() {
        NotFound => FSERR_NOT_FOUND,
        PermissionDenied => FSERR_PERM,
        AlreadyExists => FSERR_ALREADY_EXISTS,
        InvalidInput | InvalidData => FSERR_BAD_ARG,
        Unsupported => FSERR_UNSUPPORTED,
        _ => FSERR_IO,
    }
}

/// H-6 fix (2026-08-06): stable numeric encoding of FSERR_* codes
/// for `WalOutcome::Failure { code }`.  The WAL wire format
/// (snapshot.rs) uses u32 for compactness + endian-safety; this
/// mapping is a hard-fork surface (item #9 in the catalog) —
/// DO NOT reorder or renumber existing codes.  New codes append
/// at the end.
///
/// `0` is reserved as "unknown" so an in-code error slipping
/// through the mapping still round-trips deterministically
/// rather than silently mis-classifying.
pub const FSERR_CODE_UNKNOWN: u32 = 0;
pub const FSERR_CODE_BAD_ARG: u32 = 1;
pub const FSERR_CODE_IO: u32 = 2;
pub const FSERR_CODE_NOT_FOUND: u32 = 3;
pub const FSERR_CODE_ALREADY_EXISTS: u32 = 4;
pub const FSERR_CODE_PERM: u32 = 5;
pub const FSERR_CODE_UNSUPPORTED: u32 = 6;
pub const FSERR_CODE_QUARANTINE: u32 = 7;
pub const FSERR_CODE_CLOSED: u32 = 8;
pub const FSERR_CODE_BUSY: u32 = 9;
pub const FSERR_CODE_QUOTA_EXCEEDED: u32 = 10;
pub const FSERR_CODE_CROSS_DEVICE: u32 = 11;
/// Slice-8b `wait: true` cancellation — appended (code 12) per the
/// "new codes append at the end" convention.  DO NOT reorder or
/// renumber existing codes.
pub const FSERR_CODE_CANCELLED: u32 = 12;

/// Map a spec-canonical FSERR string to its stable u32 code for
/// on-wire encoding in the WAL.  Unknown / non-canonical inputs
/// return `FSERR_CODE_UNKNOWN` (never panics — a hostile or
/// out-of-band error string still round-trips deterministically).
pub fn fserr_to_code(s: &str) -> u32 {
    match s {
        FSERR_BAD_ARG => FSERR_CODE_BAD_ARG,
        FSERR_IO => FSERR_CODE_IO,
        FSERR_NOT_FOUND => FSERR_CODE_NOT_FOUND,
        FSERR_ALREADY_EXISTS => FSERR_CODE_ALREADY_EXISTS,
        FSERR_PERM => FSERR_CODE_PERM,
        FSERR_UNSUPPORTED => FSERR_CODE_UNSUPPORTED,
        FSERR_QUARANTINE => FSERR_CODE_QUARANTINE,
        FSERR_CLOSED => FSERR_CODE_CLOSED,
        FSERR_BUSY => FSERR_CODE_BUSY,
        FSERR_QUOTA_EXCEEDED => FSERR_CODE_QUOTA_EXCEEDED,
        FSERR_CROSS_DEVICE => FSERR_CODE_CROSS_DEVICE,
        FSERR_CANCELLED => FSERR_CODE_CANCELLED,
        _ => FSERR_CODE_UNKNOWN,
    }
}
