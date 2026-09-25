// String error codes returned to Rholang callers as the second element
// of `[false, code, msg]` responses.
//
// Both the caller-facing `FSERR_*: FserrCode` const and the WAL-wire
// `FSERR_CODE_*: u32` const are declared in a single
// `consensus_error_codes!` invocation below.  The macro emits four
// artifacts from one source of truth:
//
//   1. `pub const FSERR_<NAME>: FserrCode = FserrCode("FSERR_<NAME>");`
//      for caller-facing pattern-match ergonomics.
//   2. `pub const FSERR_CODE_<NAME>: u32 = <code>;` for compact
//      WAL wire encoding.
//   3. `pub fn fserr_to_code(&str) -> u32` — the taxonomy bridge.
//   4. A `const _: () = { ... }` compile-time assertion that the
//      declared codes are contiguous from 1 up to N (with 0
//      reserved as UNKNOWN).
//
// DO NOT reorder or renumber existing codes.  The u32 mapping is a
// consensus surface: a downstream fingerprint fold picks up any drift,
// but the string/int coherence is best pinned at the source.

use std::io;

use paste::paste;

/// Typed FSERR code, a newtype over `&'static str`.  The compiler
/// catches "someone passed a raw string where a canonical error code
/// was expected" at every call site that used to accept any
/// `&'static str`.
///
/// # Invariants
///
/// - The inner `&'static str` must match the identifier name
///   spec-canonical (`"FSERR_BAD_ARG"` for `FSERR_BAD_ARG`).  The
///   `consensus_error_codes!` macro enforces this at declaration
///   time via `stringify!`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FserrCode(pub &'static str);

impl FserrCode {
    pub const fn as_str(&self) -> &'static str { self.0 }
}

impl std::fmt::Display for FserrCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.0) }
}

impl PartialEq<&str> for FserrCode {
    fn eq(&self, other: &&str) -> bool { self.0 == *other }
}

impl PartialEq<str> for FserrCode {
    fn eq(&self, other: &str) -> bool { self.0 == other }
}

impl PartialEq<FserrCode> for &str {
    fn eq(&self, other: &FserrCode) -> bool { *self == other.0 }
}

impl PartialEq<FserrCode> for str {
    fn eq(&self, other: &FserrCode) -> bool { self == other.0 }
}

impl PartialEq<String> for FserrCode {
    fn eq(&self, other: &String) -> bool { self.0 == other.as_str() }
}

impl PartialEq<FserrCode> for String {
    fn eq(&self, other: &FserrCode) -> bool { self.as_str() == other.0 }
}

/// Reserved as "unknown" so an in-code error slipping through the
/// mapping still round-trips deterministically rather than silently
/// mis-classifying.  See `fserr_to_code`.
pub const FSERR_CODE_UNKNOWN: u32 = 0;

/// Single-source-of-truth macro for the FSERR taxonomy.
///
/// Emits `pub const FSERR_BAD_ARG: FserrCode = FserrCode("FSERR_BAD_ARG");`
/// and `pub const FSERR_CODE_BAD_ARG: u32 = 1;` for each entry, plus
/// the `fserr_to_code` bridge and a compile-time contiguity check.
macro_rules! consensus_error_codes {
    ( $( ( $name:ident, $code:expr ) ),+ $(,)? ) => {
        paste! {
            $(
                pub const [<FSERR_ $name>]: FserrCode =
                    FserrCode(stringify!([<FSERR_ $name>]));
                pub const [<FSERR_CODE_ $name>]: u32 = $code;
            )+

            /// Map a spec-canonical FSERR string to its stable u32 code
            /// for on-wire encoding.  Unknown / non-canonical inputs
            /// return `FSERR_CODE_UNKNOWN` (never panics).
            pub fn fserr_to_code(s: &str) -> u32 {
                match s {
                    $(
                        s if s == [<FSERR_ $name>].as_str() => [<FSERR_CODE_ $name>],
                    )+
                    _ => FSERR_CODE_UNKNOWN,
                }
            }

            /// Compile-time contiguity check: declared codes must be
            /// exactly `[1, N]` (with 0 reserved as UNKNOWN).
            const _: () = {
                let codes: &[u32] = &[$([<FSERR_CODE_ $name>]),+];
                let n = codes.len();
                let mut i = 0;
                while i < n {
                    let expected = (i as u32) + 1;
                    assert!(
                        codes[i] == expected,
                        "FSERR codes must be contiguous 1..N with no \
                         gaps or duplicates; a mis-numbered code was \
                         declared in `consensus_error_codes!`",
                    );
                    i += 1;
                }
            };
        }
    };
}

consensus_error_codes! {
    (BAD_ARG,               1),
    (IO,                    2),
    (NOT_FOUND,             3),
    (ALREADY_EXISTS,        4),
    (PERM,                  5),
    (UNSUPPORTED,           6),
    (QUARANTINE,            7),
    (CLOSED,                8),
    (BUSY,                  9),
    (QUOTA_EXCEEDED,       10),
    (CROSS_DEVICE,         11),
    (CANCELLED,            12),
    (CONSENSUS_DIVERGENCE, 13),
    (DEADLOCK,             14),
    (REVOKED,              15),
}

/// Map a `std::io::Error` kind to a stable FSERR code.  Callers invoke
/// this at the boundary between kernel errors and Rholang reply Pars.
/// Not derived from the macro because the mapping is FROM a foreign
/// taxonomy (`io::ErrorKind`) TO ours.
pub fn io_err_code(e: &io::Error) -> FserrCode {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip pin: every declared FSERR_* string maps to the
    /// corresponding FSERR_CODE_* u32 under `fserr_to_code`.
    #[test]
    fn fserr_string_to_code_round_trip_pins_every_declared_code() {
        let pairs: &[(FserrCode, u32)] = &[
            (FSERR_BAD_ARG, FSERR_CODE_BAD_ARG),
            (FSERR_IO, FSERR_CODE_IO),
            (FSERR_NOT_FOUND, FSERR_CODE_NOT_FOUND),
            (FSERR_ALREADY_EXISTS, FSERR_CODE_ALREADY_EXISTS),
            (FSERR_PERM, FSERR_CODE_PERM),
            (FSERR_UNSUPPORTED, FSERR_CODE_UNSUPPORTED),
            (FSERR_QUARANTINE, FSERR_CODE_QUARANTINE),
            (FSERR_CLOSED, FSERR_CODE_CLOSED),
            (FSERR_BUSY, FSERR_CODE_BUSY),
            (FSERR_QUOTA_EXCEEDED, FSERR_CODE_QUOTA_EXCEEDED),
            (FSERR_CROSS_DEVICE, FSERR_CODE_CROSS_DEVICE),
            (FSERR_CANCELLED, FSERR_CODE_CANCELLED),
            (FSERR_CONSENSUS_DIVERGENCE, FSERR_CODE_CONSENSUS_DIVERGENCE),
            (FSERR_DEADLOCK, FSERR_CODE_DEADLOCK),
            (FSERR_REVOKED, FSERR_CODE_REVOKED),
        ];
        for (c, code) in pairs {
            let s = c.as_str();
            assert_eq!(fserr_to_code(s), *code);
            let expected_str = format!("FSERR_{}", &s[6..]);
            assert_eq!(s, expected_str);
        }
        assert_eq!(fserr_to_code("bogus"), FSERR_CODE_UNKNOWN);
        assert_eq!(fserr_to_code(""), FSERR_CODE_UNKNOWN);
    }

    /// Pin the `FserrCode` newtype behavior.
    #[test]
    fn fserr_code_newtype_shape_pins() {
        let code = FSERR_BAD_ARG;
        assert_eq!(code.as_str(), "FSERR_BAD_ARG");
        assert_eq!(code, "FSERR_BAD_ARG");
        assert_eq!("FSERR_BAD_ARG", code);
        assert_eq!(format!("{code}"), "FSERR_BAD_ARG");
        assert_ne!(FSERR_BAD_ARG, FSERR_IO);
        assert_ne!(FSERR_BAD_ARG.as_str(), FSERR_IO.as_str());
    }

    /// Pin the current-slice count of FSERR codes at 15.  Adding a new
    /// code is a consensus surface change and requires a coordinated
    /// peer upgrade — bumping this pin is the flag for a reviewer to
    /// verify the code was appended (not inserted) at the tail.
    #[test]
    fn fserr_code_count_is_pinned() {
        assert_eq!(FSERR_CODE_REVOKED, 15);
    }
}
