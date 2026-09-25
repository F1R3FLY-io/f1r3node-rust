// Builders for the two canonical native-reply shapes:
//   [true, ...values]           on success
//   [false, code, msg]          on error
//
// Both shapes are represented as a Rholang list (`EList`) with the head
// discriminator followed by the payload.

use models::rhoapi::Par;

use super::super::rho_type::{RhoBoolean, RhoByteArray, RhoList, RhoNumber, RhoString};

/// Type-safe wrapper around a file-descriptor value.  Introduced
/// after a signedness bug that swapped `extract_ok_u64` for
/// `extract_ok_fd` at fd extraction sites: distinguishing fd from
/// quantity at the type level turns future misuse into a compile
/// error, not a runtime WAL divergence.
///
/// # Design invariants
///
/// - **fds are opaque handles, not magnitudes.**  They MUST never be
///   exposed to Rholang for arithmetic or ordering — only for
///   equality / pattern-matching within an agent's private state.
///   Agent encapsulation is what makes it safe to represent fds as
///   `GInt(fd as i64)` on the wire.
///
/// - **Allocated fd values fit in `[0, i64::MAX]`.**  The allocator
///   (Wave 2 `FileHandleTable`) MUST NOT hand back a `u64` with the
///   high bit set.  This makes the `as i64` wire cast lossless — no
///   bit-preserve tricks, no negative encoding — and prevents the
///   `u64::MAX` case from encoding as the POSIX-traditional error
///   sentinel `-1`.  `Fd::try_from(u64)` rejects out-of-range
///   values as defense-in-depth.
///
/// - **Uniqueness is the allocator's job**, not this newtype's.
///   `Fd` wraps a *value*; two `Fd` instances with equal inner values
///   refer to the same file.  `FileHandleTable::insert_at` (Wave 2)
///   is the enforcement point — it MUST return failure on an
///   already-occupied slot.
///
/// `#[repr(transparent)]` locks the wire/ABI layout to be identical
/// to `u64` — critical for FFI at the fd-table boundary.  See the
/// compile-time transmute check just below the impl block and the
/// `fd_is_repr_transparent_over_u64` runtime layout pin.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fd(u64);

/// Error returned by `Fd::try_from` when the input violates the
/// allocator contract (value doesn't fit in `[0, i64::MAX]`).
///
/// Carries the attempted value for diagnostic logging — the
/// allocator that produced an out-of-range value has a bug, and
/// the caller may want to surface which value tripped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdOutOfRange {
    pub attempted: u64,
}

impl std::fmt::Display for FdOutOfRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "fd value {} violates the [0, i64::MAX] allocator contract",
            self.attempted
        )
    }
}

impl std::error::Error for FdOutOfRange {}

impl Fd {
    /// Unwrap into the raw `u64` for handoff to the fd-table layer.
    /// The newtype's job is to keep call sites from mixing fd with
    /// quantity values; once the fd reaches the table boundary it
    /// becomes just a u64 again.
    #[inline]
    pub fn as_u64(self) -> u64 { self.0 }

    /// Return the fd as `i64` — safe because valid `Fd` values are
    /// in `[0, i64::MAX]` (see the design invariants on the type).
    /// This is the value that goes on the Rholang wire.
    #[inline]
    pub fn as_i64(self) -> i64 { self.0 as i64 }
}

/// Fallible construction from a raw `u64`.  Rejects any value above
/// `i64::MAX` so the allocator contract is enforced at every entry
/// point that lifts an external value into an `Fd`.
///
/// The allocator (Wave 2 `FileHandleTable`) MUST arrange to produce
/// values in `[0, i64::MAX]` and unwrap this without failure.
impl TryFrom<u64> for Fd {
    type Error = FdOutOfRange;
    #[inline]
    fn try_from(raw: u64) -> Result<Self, Self::Error> {
        if raw > i64::MAX as u64 {
            Err(FdOutOfRange { attempted: raw })
        } else {
            Ok(Fd(raw))
        }
    }
}

/// Fallible construction from a Rholang-wire `i64` — the same
/// contract as [`Fd::try_from`]`(u64)`, but rejects negatives instead
/// of the upper-`u64` half.  Convenience for extractors that pull
/// `i64` from `RhoNumber::unapply`.
impl TryFrom<i64> for Fd {
    type Error = FdOutOfRange;
    #[inline]
    fn try_from(raw: i64) -> Result<Self, Self::Error> {
        if raw < 0 {
            Err(FdOutOfRange {
                attempted: raw as u64,
            })
        } else {
            Ok(Fd(raw as u64))
        }
    }
}

// Compile-time layout pin: `Fd` and `u64` MUST have the same size
// or these transmutes fail to typecheck.  Guarantees the property
// the `#[repr(transparent)]` attribute exists to guarantee — the
// `as u64` / `as i64` casts in `ok_fd`, `extract_ok_fd`, and any
// FFI at the fd-table boundary stay sound.  (A plain
// `struct Fd(u64)` without the attribute also happens to satisfy
// size/align equality on current Rust, so this pin does not catch
// removal of the attribute itself; use it as a layout guarantee,
// not an attribute guarantee.)
#[allow(dead_code)]
const _FD_LAYOUT_MATCHES_U64: fn() = || {
    let _: u64 = unsafe { std::mem::transmute::<Fd, u64>(Fd(0)) };
    let _: Fd = unsafe { std::mem::transmute::<u64, Fd>(0) };
};

/// Stream-terminator marker string emitted by [`err_eos`].  Hoisted
/// to a named constant so a follow-up that introduces a second
/// control terminator can grep for the identifier rather than the
/// raw literal, and so accidental drift (`"EOS "` / `"eos"`) is a
/// compile error rather than a wire-format divergence.
pub const EOS_MARKER: &str = "EOS";

pub fn ok_bare() -> Par { RhoList::create_par(vec![RhoBoolean::create_par(true)]) }

pub fn ok_int(n: i64) -> Par {
    RhoList::create_par(vec![RhoBoolean::create_par(true), RhoNumber::create_par(n)])
}

/// Emit a `u64` quantity (bytes written, file offset, entry count,
/// etc.).  Rholang integers are 64-bit signed, so values above
/// `i64::MAX` saturate at the cap rather than bit-reinterpret to a
/// negative — well-formed quantities from callers with a
/// `MAX_WRITE_BYTES < i64::MAX` upstream limit fit comfortably; the
/// saturating cap is a defense-in-depth for anything that leaks
/// through.  Receivers use `extract_ok_u64`'s reject-negative guard
/// as the reciprocal enforcement on the wire.
///
/// Not to be used for file descriptors — the `Fd` newtype (with
/// `ok_fd`) is the fd-typed emitter, and it enforces the
/// `[0, i64::MAX]` allocator contract at construction time so its
/// wire cast is lossless without any cap.
///
/// TODO: once Rholang supports unsigned 64-bit integers natively,
/// this cap can be removed and the value emitted losslessly.
pub fn ok_u64(n: u64) -> Par {
    let capped = if n > i64::MAX as u64 {
        i64::MAX
    } else {
        n as i64
    };
    ok_int(capped)
}

/// Emission-side companion to `extract_ok_fd`.  Wire shape is
/// `[true, GInt(fd_as_i64)]`.  The cast is lossless because valid
/// `Fd` values are in `[0, i64::MAX]` per the allocator contract
/// (see [`Fd`] design invariants) — `TryFrom<u64>` / `TryFrom<i64>`
/// enforce it at every entry point.  No bit-preserve reinterpret,
/// no negative-encoded wire value.
///
/// The `Fd` newtype is what makes the fd/quantity signature
/// distinction load-bearing: a call site that hands a `Fd` to
/// `ok_u64` (or a raw quantity to `ok_fd`) is a compile error.
pub fn ok_fd(fd: Fd) -> Par {
    // Lossless direct cast — `fd.as_i64()` cannot be negative.
    ok_int(fd.as_i64())
}

pub fn ok_bool(b: bool) -> Par {
    RhoList::create_par(vec![
        RhoBoolean::create_par(true),
        RhoBoolean::create_par(b),
    ])
}

pub fn ok_bytes(bytes: Vec<u8>) -> Par {
    RhoList::create_par(vec![
        RhoBoolean::create_par(true),
        RhoByteArray::create_par(bytes),
    ])
}

pub fn ok_string(s: impl Into<String>) -> Par {
    RhoList::create_par(vec![
        RhoBoolean::create_par(true),
        RhoString::create_par(s.into()),
    ])
}

pub fn ok_par(p: Par) -> Par { RhoList::create_par(vec![RhoBoolean::create_par(true), p]) }

/// Wrap `items` in a nested list: `[true, [items...]]`, NOT
/// `[true, items...]`.  The inner list is a single Par slot, so
/// receivers extract items via `[_, inner]` and then iterate.
pub fn ok_list(items: Vec<Par>) -> Par {
    RhoList::create_par(vec![
        RhoBoolean::create_par(true),
        RhoList::create_par(items),
    ])
}

/// Construct a `[false, code, msg]` error reply Par.  `code` is
/// typed as `FserrCode` (the newtype over `&'static str`) so the
/// compiler rejects raw-string misuse at every call site — any
/// code passed here must be a spec-canonical `FSERR_*` constant.
///
/// Wire bytes unchanged from a pre-newtype `&'static str` signature:
/// `FserrCode::as_str()` returns the same `&'static str` value, and
/// `RhoString::create_par` encodes it identically.
pub fn err(code: super::errors::FserrCode, msg: impl Into<String>) -> Par {
    RhoList::create_par(vec![
        RhoBoolean::create_par(false),
        RhoString::create_par(code.as_str().to_string()),
        RhoString::create_par(msg.into()),
    ])
}

/// End-of-stream terminator for streaming replies (`entriesStreamNext`).
/// Deliberately 2-element (`[false, EOS_MARKER]`) so callers can
/// distinguish normal termination from a genuine error
/// (`[false, code, msg]` = 3-element).  `EOS_MARKER` is not an
/// `FSERR_*` code — EOS is expected control flow, not a failure.
pub fn err_eos() -> Par {
    RhoList::create_par(vec![
        RhoBoolean::create_par(false),
        RhoString::create_par(EOS_MARKER.to_string()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Fd layout pins -------------------------------------------

    /// Runtime layout pin.  A refactor that changes `Fd`'s size or
    /// alignment to differ from `u64` trips here.  Note: this pin
    /// does NOT catch removal of `#[repr(transparent)]` per se —
    /// a plain `struct Fd(u64)` has the same size/align as `Fd`
    /// with the attribute.  The compile-time transmute check
    /// `_FD_LAYOUT_MATCHES_U64` at the module level catches size
    /// divergences at CI-fail time; this pin exists to make the
    /// layout invariant explicit in test output.
    #[test]
    fn fd_is_repr_transparent_over_u64() {
        assert_eq!(std::mem::size_of::<Fd>(), std::mem::size_of::<u64>());
        assert_eq!(std::mem::align_of::<Fd>(), std::mem::align_of::<u64>());
    }

    // --- ok_* / err builder shape pins ----------------------------

    #[test]
    fn ok_bare_is_1_element_true_only() {
        let par = ok_bare();
        let expected = RhoList::create_par(vec![RhoBoolean::create_par(true)]);
        assert_eq!(par, expected);
    }

    #[test]
    fn ok_int_pins_shape() {
        for n in [i64::MIN, -1, 0, 1, 42, i64::MAX] {
            let par = ok_int(n);
            let expected =
                RhoList::create_par(vec![RhoBoolean::create_par(true), RhoNumber::create_par(n)]);
            assert_eq!(par, expected, "ok_int({n}) shape mismatch");
        }
    }

    #[test]
    fn ok_bool_pins_shape() {
        for b in [true, false] {
            let par = ok_bool(b);
            let expected = RhoList::create_par(vec![
                RhoBoolean::create_par(true),
                RhoBoolean::create_par(b),
            ]);
            assert_eq!(par, expected, "ok_bool({b}) shape mismatch");
        }
    }

    #[test]
    fn ok_bytes_pins_shape() {
        let bytes = vec![1u8, 2, 3, 4];
        let par = ok_bytes(bytes.clone());
        let expected = RhoList::create_par(vec![
            RhoBoolean::create_par(true),
            RhoByteArray::create_par(bytes),
        ]);
        assert_eq!(par, expected);
    }

    #[test]
    fn ok_string_pins_shape_and_accepts_into_string() {
        let par_from_str = ok_string("hello");
        let par_from_string = ok_string(String::from("hello"));
        let expected = RhoList::create_par(vec![
            RhoBoolean::create_par(true),
            RhoString::create_par("hello".to_string()),
        ]);
        assert_eq!(par_from_str, expected);
        assert_eq!(par_from_string, expected);
    }

    #[test]
    fn ok_par_pins_shape() {
        let inner = RhoNumber::create_par(42);
        let par = ok_par(inner.clone());
        let expected = RhoList::create_par(vec![RhoBoolean::create_par(true), inner]);
        assert_eq!(par, expected);
    }

    /// Explicitly pin the nesting: `ok_list([a, b, c])` produces
    /// `[true, [a, b, c]]` — the items live in a single inner-list
    /// slot, NOT spread as `[true, a, b, c]`.  A receiver that
    /// mistakenly does `[_, x, y, z]` matching would then see only
    /// the first item.  Locked in with an explicit expected-Par
    /// comparison AND an outer-length assertion.
    #[test]
    fn ok_list_wraps_items_in_nested_list() {
        let items = vec![
            RhoNumber::create_par(1),
            RhoNumber::create_par(2),
            RhoNumber::create_par(3),
        ];
        let par = ok_list(items.clone());
        let expected = RhoList::create_par(vec![
            RhoBoolean::create_par(true),
            RhoList::create_par(items.clone()),
        ]);
        assert_eq!(par, expected);

        // Outer list has exactly 2 slots: the boolean and the inner list.
        let outer = RhoList::unapply(&par).expect("outer is a list");
        assert_eq!(outer.len(), 2, "ok_list must be a 2-slot outer list");
        assert_eq!(outer[0], RhoBoolean::create_par(true));

        // Inner list carries the items in order.
        let inner = RhoList::unapply(&outer[1]).expect("inner is a list");
        assert_eq!(inner, items);
    }

    #[test]
    fn err_pins_3_element_shape() {
        let par = err(super::super::errors::FSERR_BAD_ARG, "invalid path");
        let expected = RhoList::create_par(vec![
            RhoBoolean::create_par(false),
            RhoString::create_par("FSERR_BAD_ARG".to_string()),
            RhoString::create_par("invalid path".to_string()),
        ]);
        assert_eq!(par, expected);

        // Explicitly assert the 3-element length that distinguishes
        // err() from err_eos().
        let outer = RhoList::unapply(&par).expect("err is a list");
        assert_eq!(outer.len(), 3);
    }

    /// EOS is a 2-element `[false, EOS_MARKER]` shape — deliberately
    /// distinct from error's 3-element `[false, code, msg]`, so
    /// receivers can tell "normal end-of-stream" apart from "actual
    /// error" by list length alone.  Regression risk: a follow-up
    /// that "helpfully" pads err_eos to 3 elements would silently
    /// break this discriminator.
    #[test]
    fn err_eos_is_distinct_2_element_form() {
        let eos = err_eos();
        let expected = RhoList::create_par(vec![
            RhoBoolean::create_par(false),
            RhoString::create_par(EOS_MARKER.to_string()),
        ]);
        assert_eq!(eos, expected);

        // Length-based discriminator vs. err().
        let eos_len = RhoList::unapply(&eos).expect("eos is a list").len();
        let err_len = RhoList::unapply(&err(super::super::errors::FSERR_BAD_ARG, "x"))
            .expect("err is a list")
            .len();
        assert_eq!(eos_len, 2, "err_eos MUST be 2-element");
        assert_eq!(err_len, 3, "err MUST be 3-element");
        assert_ne!(
            eos_len, err_len,
            "err_eos and err MUST be distinguishable by length"
        );

        // EOS_MARKER string is `"EOS"` — pin the exact bytes since a
        // follow-up that changed the marker would silently break every
        // stream consumer.
        assert_eq!(EOS_MARKER, "EOS");
    }

    // --- Fd construction contract ---------------------------------

    /// The allocator (Wave 2 `FileHandleTable`) MUST produce fds in
    /// `[0, i64::MAX]`.  `Fd::try_from(u64)` enforces that at every
    /// entry point: an out-of-range value fails construction with a
    /// `FdOutOfRange` error carrying the attempted value.
    #[test]
    fn fd_try_from_u64_rejects_out_of_range() {
        // Boundary: exactly at the cap succeeds.
        assert!(Fd::try_from(0u64).is_ok());
        assert!(Fd::try_from(i64::MAX as u64).is_ok());

        // Just past the cap fails.
        let over = (i64::MAX as u64).wrapping_add(1);
        assert_eq!(
            Fd::try_from(over),
            Err(FdOutOfRange { attempted: over }),
            "any u64 with the high bit set violates the allocator contract"
        );

        // Full u64::MAX (traditional POSIX -1 when reinterpreted) fails.
        assert_eq!(
            Fd::try_from(u64::MAX),
            Err(FdOutOfRange {
                attempted: u64::MAX,
            }),
            "u64::MAX must not construct — it would wire-encode as -1"
        );
    }

    /// `Fd::try_from(i64)` is a convenience for extractors that pull
    /// `i64` off the wire via `RhoNumber::unapply`.  It rejects
    /// negatives — a valid fd on the wire is always non-negative.
    #[test]
    fn fd_try_from_i64_rejects_negatives() {
        assert!(Fd::try_from(0i64).is_ok());
        assert!(Fd::try_from(1i64).is_ok());
        assert!(Fd::try_from(i64::MAX).is_ok());

        assert!(Fd::try_from(-1i64).is_err());
        assert!(Fd::try_from(i64::MIN).is_err());
    }

    // --- ok_fd / ok_u64 wire-format pins --------------------------

    /// Wire-format agreement everywhere `Fd` can exist: `ok_fd(fd)`
    /// is byte-identical to `ok_u64(fd.as_u64())` for every valid
    /// `Fd`.  Valid `Fd` values are in `[0, i64::MAX]`, so `ok_u64`'s
    /// saturating cap never fires and `ok_fd`'s cast is lossless —
    /// the two builders produce the same Par.  A future divergence
    /// here (e.g., because the allocator contract widened) needs a
    /// deliberate re-review of the extractor / newtype design.
    #[test]
    fn ok_fd_and_ok_u64_produce_identical_wire() {
        for raw in [
            0u64,
            1,
            100,
            1_000_000,
            (i64::MAX as u64) - 1,
            i64::MAX as u64,
        ] {
            let fd = Fd::try_from(raw).expect("in-range");
            assert_eq!(
                ok_fd(fd),
                ok_u64(raw),
                "ok_fd and ok_u64 must agree for valid fd {raw}"
            );
        }
    }

    /// `ok_u64` still saturates at `i64::MAX` for quantity callers
    /// that pass a raw `u64` above the cap (e.g., a bytes-written
    /// return that violated its upstream `MAX_WRITE_BYTES` cap).
    /// This is independent of the fd contract — quantities can
    /// legitimately hit the cap; fds cannot construct at all above
    /// it, per `Fd::try_from`.
    ///
    /// TODO: remove this saturating cap once Rholang gains native
    /// unsigned 64-bit integer support and `ok_u64` can emit the
    /// value losslessly.
    #[test]
    fn ok_u64_saturates_at_i64_max_for_quantities() {
        let over = (i64::MAX as u64).wrapping_add(1);
        assert_eq!(ok_u64(over), ok_int(i64::MAX));
        assert_eq!(ok_u64(u64::MAX), ok_int(i64::MAX));
    }
}
