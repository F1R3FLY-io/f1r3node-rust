//! # `ColdStoreDecode` — the cold-store DECODE boundary
//!
//! ## The hazard this trait exists to close
//!
//! bincode 1.3.3 has **no recursion limit**. Neither does bincode 2.0.1 nor
//! bincode-next 3.1.1 — both offer only a *byte* limit, defaulting to
//! `NoLimit`. The derived `Deserialize` for the `Par` family is therefore a
//! recursive descent whose native stack is Θ(term depth), measured at 28,362
//! B/level debug and 12,894 B/level release, i.e. a maximum survivable depth of
//! 73 / 161 on a 2 MiB worker thread.
//!
//! That makes `Par` decode the **shallowest** member of the Θ(depth) family —
//! 1.8× (debug) to 4.5× (release) below `<Par as Clone>::clone` — and the only
//! one whose failure is **permanent and replicated**:
//!
//! ```text
//!   peer ──▶ rspace_importer ──▶ LMDB cold store        (bytes written; never deep-decoded)
//!                                    │
//!                                    ▼
//!                            history read-back ──▶ derived Deserialize ──▶ SIGSEGV
//!                                    ▲                                        │
//!                                    └──────── every restart, every peer ─────┘
//! ```
//!
//! `rspace_importer` writes cold-store bytes to LMDB **without ever
//! deep-decoding them**, so a too-deep datum enters storage through a path that
//! structurally cannot observe its depth, and then aborts the node on *every*
//! read-back, on *every* restart, on *every* peer that synced the same state.
//! Every other Θ(depth) member is a transient, per-worker fault; this one is
//! not.
//!
//! ## Why a trait rather than a free function
//!
//! `rspace++` cannot name `Par`. `models` depends on `rspace_plus_plus`
//! (`models/Cargo.toml`), so the reverse edge would be a cycle. The
//! cold-store decode sites live here, in `rspace++`; the decoder lives in
//! `models` (`models/src/rust/rholang/bincode_decoder.rs`). A trait declared on
//! this side and implemented on the other is the only shape that respects that
//! edge.
//!
//! ## ⚠ Why there is deliberately NO blanket impl
//!
//! The obvious convenience —
//!
//! ```ignore
//! impl<T: serde::de::DeserializeOwned> ColdStoreDecode for T { … }   // ⚠ DO NOT
//! ```
//!
//! — would be a **trap**, and the reason is a language limitation, not a style
//! preference. Rust has no specialization on stable, so a blanket impl over
//! `DeserializeOwned` makes `Par` **un-overridable**: `Par` implements
//! `DeserializeOwned` (the derive is retained as a test oracle), so it would be
//! caught by the blanket, the machine impl would be a coherence error, and the
//! recursive path would silently remain in production while every call site
//! *looked* converted.
//!
//! The cost of not having it is four one-line delegations for the rspace++ test
//! doubles ([`legacy_prefix`] below) and four machine impls in `models`. The
//! benefit is that "which types are decoded by the machine" is an explicit,
//! greppable list rather than an inference outcome.
//!
//! ## The bound sites — the full enumeration
//!
//! Converting the four `decode_*` in `serializers.rs` propagates through every
//! `C`/`P`/`A`/`K`-generic layer above them. **53 bound sites in 8 files**
//! changed from `for<'a> Deserialize<'a>` to [`ColdStoreDecode`]:
//!
//! | file | sites |
//! |---|---:|
//! | `rspace++/src/rspace/rspace.rs` | 12 |
//! | `rspace++/src/rspace/history/history_repository_impl.rs` | 8 |
//! | `rspace++/src/rspace/history/instances/rspace_history_reader_impl.rs` | 8 |
//! | `rspace++/src/rspace/reporting_rspace.rs` | 8 |
//! | `rspace++/src/rspace/serializers/serializers.rs` (the `decode_*` themselves) | 5 |
//! | `rspace++/src/rspace/history/history_repository.rs` | 4 |
//! | `rspace++/src/rspace/merger/state_change.rs` | 4 |
//! | `casper/src/rust/merging/deploy_chain_index.rs` | 4 |
//! | **total** | **53** |
//!
//! (The Steps-C+D commit message states 48; that figure omitted the five sites
//! in `serializers.rs` itself. The code was, and is, 53.)
//!
//! In all six generic-plumbing files the `serde::Deserialize` **import** became
//! unused as a result — which is the compiler confirming the swap was total
//! rather than additive, and is worth more than the count itself.
//!
//! Four further sites, in
//! `shared/src/rust/store/key_value_typed_store_impl.rs`, are deliberately
//! **untouched**: that is a general typed key/value store, its generic bincode
//! path has no instantiation anywhere in the workspace (the two concrete stores
//! override `encode`/`decode`, and the report store uses `prost`, which *does*
//! enforce a recursion limit), and nothing it holds contains a `Par`.
//!
//! ## The obligation: LANGUAGE IDENTITY, not byte identity
//!
//! The encoder is **not touched**. `Serialize` stays derived,
//! `CandidateOrderingBytes` (replay-visible COMM selection) is encode-only and
//! untouched. Byte identity of the *encoding* is therefore preserved by
//! construction, and nothing about it is this trait's business.
//!
//! What the decoder owes is that it recognises **the same language**: for every
//! byte string `b`,
//!
//! ```text
//!     T::cold_decode(b)   and   bincode::deserialize::<T>(b)
//! ```
//!
//! agree — the same `Ok` value, or both `Err`. The `Err` half is not a
//! formality: the **rejection set is consensus-visible**. A node that accepts a
//! byte string another node rejects forks.
//! `models/tests/bincode_decoder_malformed.rs` pins that half by truncating
//! every corpus encoding at every byte offset, flipping bool bytes, pushing
//! `Option` tags and variant indices out of range, and setting lengths to
//! `usize::MAX`, asserting `Ok`/`Err` **agreement** (never message equality).
//!
//! ## Prefix semantics, and why the primitive is `cold_decode_prefix`
//!
//! `Datum<A>` is `{ a: A, persist: bool, source: Produce }` — `A` is a
//! **prefix** of the datum encoding, and bincode's format is not
//! self-delimiting from the outside: to read `persist` you must know where `a`
//! ended, and the only way to know that is to have parsed `a`. The primitive is
//! therefore prefix-shaped, and [`ColdStoreDecode::cold_decode`] is derived
//! from it.
//!
//! Trailing bytes are **permitted** at the top level: `bincode::deserialize`
//! uses `DefaultOptions::new().with_fixint_encoding().allow_trailing_bytes()`
//! (bincode 1.3.3 `src/lib.rs:177-185`), so tightening this to "must consume
//! the whole buffer" would *narrow* the accepted language and fork.

use std::fmt;

use serde::Deserialize;

/// The decoder's error type.
///
/// Variants mirror the `bincode::ErrorKind` cases the derived path can produce
/// for this schema, so that a reader can check the rejection sets against each
/// other by inspection. Equality of *messages* is explicitly NOT part of the
/// contract (bincode's are `Box<ErrorKind>` with `Display` text); equality of
/// `Ok`/`Err` **disposition** is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColdStoreDecodeError {
    /// The stream ended inside a value. Mirrors bincode's
    /// `ErrorKind::Io(UnexpectedEof)`, which `SliceReader` raises whenever a
    /// read would pass the end of the slice.
    UnexpectedEof {
        /// What the decoder was trying to read.
        wanted: &'static str,
        /// How many more bytes it needed.
        needed: usize,
        /// How many were left.
        available: usize,
    },
    /// A `bool` byte outside `{0, 1}`. Mirrors
    /// `ErrorKind::InvalidBoolEncoding`.
    InvalidBoolEncoding(u8),
    /// An `Option` tag byte outside `{0, 1}`. Mirrors
    /// `ErrorKind::InvalidTagEncoding`.
    InvalidTagEncoding(usize),
    /// A oneof/enum variant index outside `0..count`. The derived path reaches
    /// this through `serde::de::Error::invalid_value` on the
    /// `U32Deserializer`, which bincode surfaces as `ErrorKind::Custom`.
    InvalidVariantIndex {
        /// The enum whose index was out of range.
        type_name: &'static str,
        /// The index read from the stream.
        index: u32,
        /// How many variants the enum has.
        count: u32,
    },
    /// A `String` field whose bytes are not valid UTF-8. Mirrors
    /// `ErrorKind::InvalidUtf8Encoding`.
    InvalidUtf8Encoding,
    /// A `u64` length that does not fit in a `usize`. Mirrors bincode's
    /// `cast_u64_to_usize` `ErrorKind::Custom` (`src/config/int.rs:593`).
    ///
    /// ⚠ On a 64-bit target `usize::MAX == u64::MAX`, so this arm is
    /// unreachable there and an oversized length instead surfaces as
    /// [`ColdStoreDecodeError::UnexpectedEof`] — which is exactly what the
    /// derived path does, because `size_hint::cautious` caps its
    /// pre-allocation and the element loop then hits the end of the slice.
    /// The arm exists so a 32-bit target agrees too.
    LengthOverflow(u64),
    /// The delegating (bincode-backed) decode of a bounded type failed. Carries
    /// bincode's own message.
    Legacy(String),
    /// A machine invariant was violated — a value stack was empty where the
    /// opcode program guarantees a value. **Unreachable by construction**; it
    /// is an error rather than a panic so that a hypothetical defect degrades a
    /// single read into a `Result` instead of aborting the node, which is the
    /// entire point of this work.
    MachineInvariant(&'static str),
}

impl fmt::Display for ColdStoreDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColdStoreDecodeError::UnexpectedEof {
                wanted,
                needed,
                available,
            } => write!(
                f,
                "unexpected end of input while reading {}: needed {} more byte(s), {} available",
                wanted, needed, available
            ),
            ColdStoreDecodeError::InvalidBoolEncoding(b) => {
                write!(f, "invalid bool encoding: {}", b)
            }
            ColdStoreDecodeError::InvalidTagEncoding(t) => {
                write!(f, "invalid Option tag encoding: {}", t)
            }
            ColdStoreDecodeError::InvalidVariantIndex {
                type_name,
                index,
                count,
            } => write!(
                f,
                "invalid variant index {} for {}: expected 0 <= i < {}",
                index, type_name, count
            ),
            ColdStoreDecodeError::InvalidUtf8Encoding => write!(f, "string is not valid utf8"),
            ColdStoreDecodeError::LengthOverflow(n) => {
                write!(f, "invalid size {}: sizes must fit in a usize", n)
            }
            ColdStoreDecodeError::Legacy(msg) => write!(f, "{}", msg),
            ColdStoreDecodeError::MachineInvariant(what) => write!(
                f,
                "cold-store decoder machine invariant violated ({}) — this is a decoder defect",
                what
            ),
        }
    }
}

impl std::error::Error for ColdStoreDecodeError {}

/// Decode a value of `Self` from cold-store bytes with **`O(1)` native stack in
/// term depth**.
///
/// See the module documentation for the hazard, the no-blanket-impl rule, and
/// the language-identity obligation.
pub trait ColdStoreDecode: Sized {
    /// Decode a value from the **start** of `bytes`, returning it together with
    /// the number of bytes consumed.
    ///
    /// This is the primitive because `Datum<A>` and `WaitingContinuation<P, K>`
    /// embed their type parameters as *prefixes* of a longer encoding; see the
    /// module documentation.
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError>;

    /// Decode a value from `bytes`, **permitting trailing bytes** — the
    /// `allow_trailing_bytes()` half of `bincode::deserialize`'s configuration.
    /// Tightening this would narrow the accepted language and fork.
    fn cold_decode(bytes: &[u8]) -> Result<Self, ColdStoreDecodeError> {
        Self::cold_decode_prefix(bytes).map(|(value, _consumed)| value)
    }
}

/// The delegating prefix decode for **bounded-depth** types: run bincode over
/// an `io::Cursor` and report the cursor's final position as the consumed
/// count.
///
/// This is the body of every rspace++ test-double impl. It is sound *only* for
/// types whose maximum nesting is fixed by their own definition — the rspace++
/// instantiations are `String`, `Pattern`, `GuardedContinuation` and
/// `StringsCaptor`, none of which contains `Par` or any other recursive
/// type — so bincode's recursive descent is `O(1)` stack for them.
///
/// ⚠ Do NOT use this for a type that can nest arbitrarily. That is exactly the
/// hazard the trait exists to close.
///
/// The configuration is `bincode::deserialize`'s
/// (`DefaultOptions::new().with_fixint_encoding().allow_trailing_bytes()`,
/// bincode 1.3.3 `src/lib.rs:177-185`) plus **one addition that is required for
/// correctness, not for taste** — see below.
///
/// ## ⚠ Why `.with_limit(bytes.len())` is mandatory here
///
/// Consumption has to be *observable*, which forces `deserialize_from` over an
/// `io::Cursor` (bincode's `SliceReader` keeps its remaining-length private, so
/// the slice API cannot report a prefix length). But bincode's two readers do
/// **not** agree on malformed input:
///
/// | reader | huge `Vec<u8>`/`String` length | behaviour |
/// |--------|-------------------------------|-----------|
/// | `SliceReader` (`bincode::deserialize`) | `get_byte_slice` checks `length > slice.len()` **first** | clean `Err(UnexpectedEof)` |
/// | `IoReader` (`deserialize_from`) | `fill_buffer` does `temp_buffer.resize(length, 0)` **first** | allocates `length` bytes → abort |
///
/// A `String` payload prefixed with `[0xFF; 8]` would therefore be a clean
/// rejection on the derived path and an out-of-memory abort here — precisely
/// the class of divergence this whole change exists to remove.
///
/// The byte limit closes it: `Deserializer::read_vec` calls
/// `self.options.limit().add(len)` **before** `reader.get_byte_buffer(len)`
/// (bincode 1.3.3 `src/de/mod.rs:93-97`), so an oversized length trips
/// `ErrorKind::SizeLimit` before any allocation. And it cannot reject anything
/// the derived path accepts: every read that advances the cursor also charges
/// the limit exactly its own width (`read_literal_type::<T>` charges
/// `size_of::<T>()`; `read_bytes(n)` charges `n`), so the charge is at most the
/// bytes consumed, and the budget starts at `bytes.len()`.
pub fn legacy_prefix<T>(bytes: &[u8]) -> Result<(T, usize), ColdStoreDecodeError>
where T: for<'a> Deserialize<'a> {
    use std::io::Cursor;

    use bincode::Options;

    let mut cursor = Cursor::new(bytes);
    let options = bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(bytes.len() as u64);
    let value: T = options
        .deserialize_from(&mut cursor)
        .map_err(|e| ColdStoreDecodeError::Legacy(e.to_string()))?;
    Ok((value, cursor.position() as usize))
}

/// Read a bincode `u64` length prefix from the start of `bytes`, returning the
/// length and the number of bytes consumed (always 8).
///
/// `IntEncoding::deserialize_len` narrows the `u64` to a `usize`
/// (`bincode/src/config/int.rs:69-73`); on a 64-bit target that never fails and
/// an oversized length is caught by the element loop running out of input,
/// which is exactly where the derived path catches it.
pub fn read_len_prefix(bytes: &[u8]) -> Result<(usize, usize), ColdStoreDecodeError> {
    if bytes.len() < 8 {
        return Err(ColdStoreDecodeError::UnexpectedEof {
            wanted: "a u64 length prefix",
            needed: 8,
            available: bytes.len(),
        });
    }
    let raw = u64::from_le_bytes(bytes[..8].try_into().expect("8 bytes"));
    let len = usize::try_from(raw).map_err(|_| ColdStoreDecodeError::LengthOverflow(raw))?;
    Ok((len, 8))
}

/// serde 1.0.228's sequence pre-allocation cap, reproduced exactly
/// (`serde/src/core/private/size_hint.rs:12-23`:
/// `min(hint, 1 MiB / size_of::<Element>())`).
///
/// ⚠ Load-bearing, not tidiness. A `u64` length near `usize::MAX` read straight
/// into `Vec::with_capacity` is an out-of-memory **abort** — a failure mode the
/// derived path does not have, because serde caps its pre-allocation and then
/// fails on the first element that runs out of input. Diverging on the
/// *disposition* of a malformed input is the consensus-visible half of a
/// decoder; see `models/tests/bincode_decoder_malformed.rs`.
///
/// (Historical note: older serde used a flat 4,096-element cap. 1.0.228 uses
/// the byte-budget form above; the form reproduced here is the one in the
/// lockfile.)
pub fn cautious_capacity<T>(hint: usize) -> usize {
    const MAX_PREALLOC_BYTES: usize = 1024 * 1024;
    if std::mem::size_of::<T>() == 0 {
        0
    } else {
        hint.min(MAX_PREALLOC_BYTES / std::mem::size_of::<T>())
    }
}

/// Decode a `Vec<T>` — a bincode `u64` count followed by that many `T`s — from
/// the start of `bytes`, returning the vector and the bytes consumed.
pub fn cold_decode_vec_prefix<T: ColdStoreDecode>(
    bytes: &[u8],
) -> Result<(Vec<T>, usize), ColdStoreDecodeError> {
    let (count, mut consumed) = read_len_prefix(bytes)?;
    let mut out = Vec::with_capacity(cautious_capacity::<T>(count));
    for _ in 0..count {
        let (value, used) = T::cold_decode_prefix(&bytes[consumed..])?;
        consumed += used;
        out.push(value);
    }
    Ok((out, consumed))
}

/// Decode the bounded TAIL of a record whose head was decoded by the machine.
///
/// `Datum<A>` is `{ a: A, persist: bool, source: Produce }` and
/// `WaitingContinuation<P, K>` is `{ patterns, continuation, persist, peeks,
/// source }`: once the `Par`-bearing prefix is consumed, everything that
/// follows is bounded-depth and can go straight to bincode.
///
/// A bincode **tuple** is exactly the concatenation of its elements —
/// `deserialize_tuple(n)` → `visit_seq`, no framing (`bincode/src/de/mod.rs:
/// 293-330`) — and a derived struct is `deserialize_struct` →
/// `deserialize_tuple(fields.len())` (`:402-412`). So splitting a record into
/// "machine prefix" and "bincode tuple tail" is byte-identical to decoding the
/// whole struct with the derive, which is why this composition is sound rather
/// than merely convenient.
pub fn legacy_tail<T>(bytes: &[u8]) -> Result<T, ColdStoreDecodeError>
where T: for<'a> Deserialize<'a> {
    bincode::deserialize(bytes).map_err(|e| ColdStoreDecodeError::Legacy(e.to_string()))
}

/// `String` is one of the rspace++ **test-double** instantiations
/// (`RSpace<String, Pattern, String, StringsCaptor>` in
/// `rspace++/tests/storage_actions_test.rs`), and it is a `std` type, so the
/// orphan rule puts its impl here rather than in the test crate that uses it.
///
/// Bounded by definition: a `String` contains no `String`.
impl ColdStoreDecode for String {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

/// `Vec<u8>` — the raw-channel test-double instantiation used by the history
/// and exporter suites. Bounded by definition.
impl ColdStoreDecode for Vec<u8> {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}
