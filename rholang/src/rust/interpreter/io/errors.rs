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
