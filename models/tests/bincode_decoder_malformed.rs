//! # The MALFORMED-INPUT differential — pinning the REJECTION SET
//!
//! ## Why this file is the one that must not be skipped
//!
//! A decoder has two halves. The half everyone tests is "does it decode valid
//! input correctly". The half that decides whether a network stays converged is
//! **which inputs it refuses**:
//!
//! > A node that accepts a byte string another node rejects **forks**.
//!
//! Cold-store bytes arrive from peers through `rspace_importer`, which writes
//! them to LMDB without deep-decoding them. So the bytes a node reads back are
//! not necessarily bytes it produced, and "well-formed" is not an assumption
//! the decoder is entitled to make. If the machine accepted a truncated datum
//! that the derived decoder rejected — or rejected one it accepted — two nodes
//! would disagree about the contents of the same trie leaf.
//!
//! ## What is asserted, and what deliberately is not
//!
//! `Ok`/`Err` **agreement**, and on `Ok`, **value** equality. NOT message
//! equality: bincode's errors are `Box<ErrorKind>` with prose `Display` text,
//! and pinning that text would be pinning an implementation detail of a third-
//! party crate rather than the property that matters. The error *kinds* are
//! documented as a correspondence table in
//! `models/src/rust/rholang/bincode_decoder.rs` §6.
//!
//! ## The mutation families
//!
//! Each family targets one thing the derived decoder is known to reject, and is
//! applied to **every** encoding in the corpus:
//!
//! ```text
//!   family                    what it probes
//!   ───────────────────────── ─────────────────────────────────────────────
//!   truncate at every offset  UnexpectedEof at every position in the stream —
//!                             the exhaustive one, and the only family that
//!                             cannot miss a resume point
//!   bool byte -> 2            InvalidBoolEncoding
//!   Option tag -> 2           InvalidTagEncoding (a DIFFERENT error from the
//!                             bool case, though both read one byte)
//!   variant index -> N        out-of-range oneof index
//!   u64 length -> usize::MAX  the pre-allocation hazard: without serde's
//!                             `size_hint::cautious` cap this is an OOM ABORT
//!                             on one side and a clean Err on the other
//!   append trailing bytes     allow_trailing_bytes: both must ACCEPT
//!   every single-byte flip    a broad sweep over short encodings
//! ```
//!
//! The last three families cannot know where a `bool`, a tag or a length
//! actually sits without re-implementing the parser — so instead of guessing,
//! they apply the mutation at **every** byte offset and let agreement do the
//! work. A mutation that lands on an unrelated byte still has to produce the
//! same disposition on both sides, which is exactly the property under test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Mutex, MutexGuard};

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use serde::Deserialize;

mod par_corpus;
use par_corpus as corpus;

/// Largest single allocation requested since the last observation. The
/// malformed-input gate uses this to turn an allocator blow-up into a failure
/// that names the exact mutation and decoder side. Tracking requests rather
/// than RSS avoids allocator retention and page-accounting noise.
static MAX_ALLOCATION_REQUEST: AtomicUsize = AtomicUsize::new(0);
static LIVE_ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

struct TrackingSystem;

#[global_allocator]
static TEST_ALLOCATOR: TrackingSystem = TrackingSystem;

unsafe impl GlobalAlloc for TrackingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        MAX_ALLOCATION_REQUEST.fetch_max(layout.size(), AtomicOrdering::Relaxed);
        // SAFETY: this allocator is a transparent observer over `System`.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_live_growth(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        MAX_ALLOCATION_REQUEST.fetch_max(layout.size(), AtomicOrdering::Relaxed);
        // SAFETY: this allocator is a transparent observer over `System`.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_live_growth(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` came from the delegated `System` allocator.
        unsafe { System.dealloc(ptr, layout) }
        LIVE_ALLOCATED_BYTES.fetch_sub(layout.size(), AtomicOrdering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        MAX_ALLOCATION_REQUEST.fetch_max(new_size, AtomicOrdering::Relaxed);
        // SAFETY: `ptr` and `layout` came from the delegated `System` allocator.
        let resized = unsafe { System.realloc(ptr, layout, new_size) };
        if !resized.is_null() {
            if new_size >= layout.size() {
                record_live_growth(new_size - layout.size());
            } else {
                LIVE_ALLOCATED_BYTES.fetch_sub(layout.size() - new_size, AtomicOrdering::Relaxed);
            }
        }
        resized
    }
}

fn record_live_growth(bytes: usize) {
    let live = LIVE_ALLOCATED_BYTES.fetch_add(bytes, AtomicOrdering::Relaxed) + bytes;
    PEAK_ALLOCATED_BYTES.fetch_max(live, AtomicOrdering::Relaxed);
}

fn reset_max_allocation_request() { MAX_ALLOCATION_REQUEST.store(0, AtomicOrdering::Relaxed); }

fn max_allocation_request() -> usize { MAX_ALLOCATION_REQUEST.load(AtomicOrdering::Relaxed) }

fn start_heap_window() -> usize {
    let baseline = LIVE_ALLOCATED_BYTES.load(AtomicOrdering::Relaxed);
    PEAK_ALLOCATED_BYTES.store(baseline, AtomicOrdering::Relaxed);
    baseline
}

fn heap_window_peak(baseline: usize) -> usize {
    PEAK_ALLOCATED_BYTES
        .load(AtomicOrdering::Relaxed)
        .saturating_sub(baseline)
}

/// The one assertion this file makes, everywhere.
///
/// `hits` counts how many mutations produced `Err` on BOTH sides, so a family
/// that silently stopped perturbing anything can be caught by its caller
/// instead of reporting a comfortable pass.
fn assert_agree<T, F>(label: F, bytes: &[u8], rejections: &mut usize)
where
    T: ColdStoreDecode + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    F: FnOnce() -> String,
{
    // The generated machine gets the strict bound. The third-party derived
    // oracle can hold one independent 1 MiB cautious buffer for each nested
    // sequence while it recursively descends, so it receives a wider bound and
    // runs in short-lived batches to prevent freed arenas accumulating in RSS.
    const MAX_ORACLE_DECODE_ALLOCATION: usize = 64 * 1024 * 1024;
    const MAX_MACHINE_DECODE_ALLOCATION: usize = 8 * 1024 * 1024;

    reset_max_allocation_request();
    let oracle_baseline = start_heap_window();
    let oracle: Result<T, _> = bincode::deserialize(bytes);
    let oracle_max = max_allocation_request();
    let oracle_peak = heap_window_peak(oracle_baseline);
    reset_max_allocation_request();
    let machine_baseline = start_heap_window();
    let machine = T::cold_decode(bytes);
    let machine_max = max_allocation_request();
    let machine_peak = heap_window_peak(machine_baseline);
    if oracle_max > MAX_ORACLE_DECODE_ALLOCATION
        || oracle_peak > MAX_ORACLE_DECODE_ALLOCATION
        || machine_max > MAX_MACHINE_DECODE_ALLOCATION
        || machine_peak > MAX_MACHINE_DECODE_ALLOCATION
    {
        let label = label();
        panic!(
            "{label}: a malformed-input decode exceeded its heap gate \
             (oracle=64 MiB, machine=8 MiB; \
             (largest request: derived={oracle_max}, machine={machine_max}; peak live delta: \
             derived={oracle_peak}, machine={machine_peak}; input={} bytes)",
            bytes.len()
        );
    }
    match (oracle, machine) {
        (Ok(a), Ok(b)) => assert!(
            a == b,
            "{}: BOTH accepted, but produced DIFFERENT values. The cold-store \
             decoder and the derived decoder must recognise the same language.",
            label()
        ),
        (Err(_), Err(_)) => *rejections += 1,
        (Ok(a), Err(e)) => {
            let label = label();
            panic!(
                "{label}: the derived decoder ACCEPTED this byte string and the machine \
             REJECTED it ({e}). This narrows the accepted language — a node running \
             the machine would refuse state its peers accept.\n  value: {a:?}"
            )
        }
        (Err(_), Ok(b)) => {
            let label = label();
            panic!(
                "{label}: the machine ACCEPTED a byte string the derived decoder REJECTED. \
             This widens the accepted language — a node running the machine would \
             admit state its peers refuse.\n  value: {b:?}"
            )
        }
    }
}

/// The seven exhaustive mutation families are deliberately CPU-heavy and each
/// walks the same allocation-rich corpus. Libtest would otherwise launch them
/// concurrently in one process, multiplying their allocator high-water marks
/// and allowing a verification target to exhaust the host. Serialising the
/// families changes no cases and no oracle; it only bounds peak resident
/// memory. A poisoned guard is recovered so one useful failure does not hide
/// the remaining families behind seven lock-poison failures.
static MALFORMED_FAMILY: Mutex<()> = Mutex::new(());

fn malformed_family_guard() -> MutexGuard<'static, ()> {
    MALFORMED_FAMILY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Every corpus encoding, as `(label, bytes)`, for all four root types plus a
/// deep case per root.
fn encodings() -> Vec<(String, Kind, Vec<u8>)> {
    let mut out: Vec<(String, Kind, Vec<u8>)> = Vec::new();
    for (label, par) in corpus::par_corpus() {
        out.push((
            format!("Par::{label}"),
            Kind::Par,
            bincode::serialize(&par).expect("serialize"),
        ));
    }
    for (label, v) in corpus::list_par_with_random_corpus() {
        out.push((
            format!("ListParWithRandom::{label}"),
            Kind::ListParWithRandom,
            bincode::serialize(&v).expect("serialize"),
        ));
    }
    for (label, v) in corpus::bind_pattern_corpus() {
        out.push((
            format!("BindPattern::{label}"),
            Kind::BindPattern,
            bincode::serialize(&v).expect("serialize"),
        ));
    }
    for (label, v) in corpus::tagged_continuation_corpus() {
        out.push((
            format!("TaggedContinuation::{label}"),
            Kind::TaggedContinuation,
            bincode::serialize(&v).expect("serialize"),
        ));
    }
    out
}

#[derive(Clone, Copy)]
enum Kind {
    Par,
    ListParWithRandom,
    BindPattern,
    TaggedContinuation,
}

fn check<F>(label: F, kind: Kind, bytes: &[u8], rejections: &mut usize)
where F: FnOnce() -> String {
    match kind {
        Kind::Par => assert_agree::<Par, _>(label, bytes, rejections),
        Kind::ListParWithRandom => assert_agree::<ListParWithRandom, _>(label, bytes, rejections),
        Kind::BindPattern => assert_agree::<BindPattern, _>(label, bytes, rejections),
        Kind::TaggedContinuation => assert_agree::<TaggedContinuation, _>(label, bytes, rejections),
    }
}

/// Allocation-heavy mutation sweeps run in bounded batches. A child process
/// returns all of its allocator arenas to the operating system at batch end,
/// so hundreds of thousands of deliberately hostile decodes cannot accumulate
/// allocator retention in the long-lived libtest process. Every child inherits
/// the parent's cgroup, and every individual decode is additionally checked by
/// the 8 MiB allocation gate above.
/// Offsets per isolated worker. One offset expands to five byte/variant
/// mutations or six hostile-length mutations, and each mutation runs both
/// decoders. The derived decoder may retain freed arenas until the worker
/// exits, so the resource unit is the number of *decode attempts*, not merely
/// the number of offsets. Four offsets bound a worker to at most 24 mutations
/// (48 decoder invocations) while still amortising process startup. The parent
/// starts the next two-worker window only after both preceding workers exit, so
/// the operating system returns every retained arena between windows without
/// dropping or sampling any mutation.
const MUTATION_BATCH_OFFSETS: usize = 4;
/// Independent batches execute in a small fixed-width window. Two workers use
/// otherwise idle cores without multiplying the post-fix resident-set bound
/// back toward the pre-fix multi-gigabyte peak. Results are joined and folded
/// in source order, so scheduling cannot affect counts or diagnostics.
const MUTATION_BATCH_WORKERS: usize = 2;
const MUTATION_CHILD_FAMILY: &str = "BINCODE_MALFORMED_CHILD_FAMILY";
const MUTATION_CHILD_FIXTURE: &str = "BINCODE_MALFORMED_CHILD_FIXTURE";
const MUTATION_CHILD_START: &str = "BINCODE_MALFORMED_CHILD_START";
const MUTATION_CHILD_END: &str = "BINCODE_MALFORMED_CHILD_END";
const MUTATION_BATCH_RESULT: &str = "BINCODE_MALFORMED_BATCH_RESULT";

#[derive(Clone, Copy)]
enum MutationFamily {
    Byte,
    Variant,
    Length,
}

impl MutationFamily {
    fn name(self) -> &'static str {
        match self {
            Self::Byte => "byte",
            Self::Variant => "variant",
            Self::Length => "length",
        }
    }

    fn parse(name: &str) -> Self {
        match name {
            "byte" => Self::Byte,
            "variant" => Self::Variant,
            "length" => Self::Length,
            other => panic!("unknown malformed mutation family {other:?}"),
        }
    }

    fn offset_count(self, bytes_len: usize) -> usize {
        match self {
            Self::Byte => bytes_len,
            Self::Variant => bytes_len.saturating_sub(4),
            Self::Length => bytes_len.saturating_sub(8),
        }
    }
}

fn run_mutation_batches(family: MutationFamily) -> (usize, usize) {
    let executable = std::env::current_exe().expect("locate malformed-input test executable");
    let fixtures = encodings();
    let mut mutations = 0usize;
    let mut rejections = 0usize;

    for (fixture, (label, _kind, bytes)) in fixtures.iter().enumerate() {
        let count = family.offset_count(bytes.len());
        let window_stride = MUTATION_BATCH_OFFSETS * MUTATION_BATCH_WORKERS;
        for window_start in (0..count).step_by(window_stride) {
            let executable_path = executable.as_path();
            let label_text = label.as_str();
            let results = std::thread::scope(|scope| {
                let mut handles = Vec::with_capacity(MUTATION_BATCH_WORKERS);
                for worker in 0..MUTATION_BATCH_WORKERS {
                    let start = window_start + worker * MUTATION_BATCH_OFFSETS;
                    if start >= count {
                        break;
                    }
                    let end = (start + MUTATION_BATCH_OFFSETS).min(count);
                    handles.push(scope.spawn(move || {
                        run_mutation_batch(executable_path, family, fixture, label_text, start, end)
                    }));
                }
                handles
                    .into_iter()
                    .map(|handle| handle.join().expect("malformed mutation worker panicked"))
                    .collect::<Vec<_>>()
            });
            for (batch_mutations, batch_rejections) in results {
                mutations += batch_mutations;
                rejections += batch_rejections;
            }
        }
    }
    (mutations, rejections)
}

fn run_mutation_batch(
    executable: &std::path::Path,
    family: MutationFamily,
    fixture: usize,
    label: &str,
    start: usize,
    end: usize,
) -> (usize, usize) {
    let output = Command::new(executable)
        .arg("mutation_batch_child")
        .arg("--exact")
        .arg("--ignored")
        .arg("--nocapture")
        .env(MUTATION_CHILD_FAMILY, family.name())
        .env(MUTATION_CHILD_FIXTURE, fixture.to_string())
        .env(MUTATION_CHILD_START, start.to_string())
        .env(MUTATION_CHILD_END, end.to_string())
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "failed to start {} mutation child for {label}[{start}..{end}]: {error}",
                family.name()
            )
        });
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{} mutation child failed for {label}[{start}..{end}] with {}\nstdout:\n{}\nstderr:\n{}",
        family.name(),
        output.status,
        stdout,
        stderr
    );
    let line = stdout
        .lines()
        .find(|line| line.starts_with(MUTATION_BATCH_RESULT))
        .unwrap_or_else(|| {
            panic!(
                "{} mutation child for {label}[{start}..{end}] returned no result marker\nstdout:\n{stdout}\nstderr:\n{stderr}",
                family.name()
            )
        });
    let mut fields = line.split_whitespace();
    assert_eq!(fields.next(), Some(MUTATION_BATCH_RESULT));
    let mutations = fields
        .next()
        .expect("batch result mutation count")
        .parse::<usize>()
        .expect("numeric batch mutation count");
    let rejections = fields
        .next()
        .expect("batch result rejection count")
        .parse::<usize>()
        .expect("numeric batch rejection count");
    assert!(
        fields.next().is_none(),
        "unexpected fields in mutation batch result {line:?}"
    );
    (mutations, rejections)
}

fn required_child_usize(name: &str) -> usize {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("mutation child requires {name}"))
        .parse()
        .unwrap_or_else(|error| panic!("mutation child {name} is not usize: {error}"))
}

/// One resource-bounded batch driven by [`run_mutation_batches`]. Ignored so a
/// normal libtest invocation never mistakes a worker for an independent proof.
#[test]
#[ignore = "child process of the malformed-input allocation gate"]
fn mutation_batch_child() {
    let Ok(family_name) = std::env::var(MUTATION_CHILD_FAMILY) else {
        return;
    };
    let family = MutationFamily::parse(&family_name);
    let fixture = required_child_usize(MUTATION_CHILD_FIXTURE);
    let start = required_child_usize(MUTATION_CHILD_START);
    let end = required_child_usize(MUTATION_CHILD_END);
    let (label, kind, bytes) = encodings()
        .into_iter()
        .nth(fixture)
        .unwrap_or_else(|| panic!("mutation child fixture {fixture} is out of range"));
    let count = family.offset_count(bytes.len());
    assert!(
        start <= end && end <= count && end - start <= MUTATION_BATCH_OFFSETS,
        "invalid {} mutation batch {start}..{end} for {label} with {count} offsets",
        family.name()
    );

    let mut mutations = 0usize;
    let mut rejections = 0usize;
    match family {
        MutationFamily::Byte => {
            for offset in start..end {
                for replacement in [0u8, 1, 2, 0x7f, 0xff] {
                    if bytes[offset] == replacement {
                        continue;
                    }
                    let mut mutant = bytes.clone();
                    mutant[offset] = replacement;
                    mutations += 1;
                    check(
                        || format!("{label} byte[{offset}] := {replacement:#04x}"),
                        kind,
                        &mutant,
                        &mut rejections,
                    );
                }
            }
        }
        MutationFamily::Variant => {
            for offset in start..end {
                for index in [2u32, 9, 36, 37, u32::MAX] {
                    let mut mutant = bytes.clone();
                    mutant[offset..offset + 4].copy_from_slice(&index.to_le_bytes());
                    mutations += 1;
                    check(
                        || format!("{label} variant@{offset} := {index}"),
                        kind,
                        &mutant,
                        &mut rejections,
                    );
                }
            }
        }
        MutationFamily::Length => {
            let hostile: [u64; 6] = [
                u64::MAX,
                usize::MAX as u64,
                u64::MAX / 2,
                1 << 62,
                1 << 40,
                4097,
            ];
            for offset in start..end {
                for len in hostile {
                    let mut mutant = bytes.clone();
                    mutant[offset..offset + 8].copy_from_slice(&len.to_le_bytes());
                    mutations += 1;
                    check(
                        || format!("{label} len@{offset} := {len}"),
                        kind,
                        &mutant,
                        &mut rejections,
                    );
                }
            }
        }
    }
    println!("{MUTATION_BATCH_RESULT} {mutations} {rejections}");
}

// ===========================================================================
// Family 1 — truncation at EVERY byte offset
// ===========================================================================

/// The exhaustive family. Every prefix of every corpus encoding is fed to both
/// decoders. This is the only mutation that provably visits every resume point
/// in the machine: a `*Build` op that read its scalars in the wrong order, or
/// a resume point that read one byte too many, changes *where* the stream runs
/// out and therefore changes the disposition of some prefix.
///
/// ⚠ Cost is Θ(Σ len²) over the corpus, which is why the deep cases are capped
/// at depth 48 and the corpus is not the place to add megabyte fixtures.
#[test]
fn truncation_at_every_offset_agrees() {
    let _serial = malformed_family_guard();
    let mut rejections = 0usize;
    let mut prefixes = 0usize;
    for (label, kind, bytes) in encodings() {
        for cut in 0..bytes.len() {
            prefixes += 1;
            check(
                || format!("{label} truncated at {cut}"),
                kind,
                &bytes[..cut],
                &mut rejections,
            );
        }
    }
    assert!(
        prefixes > 50_000,
        "ANTI-VACUITY: only {prefixes} truncations were tried; the corpus has shrunk"
    );
    assert!(
        rejections > prefixes / 2,
        "ANTI-VACUITY: only {rejections} of {prefixes} truncations were REJECTED by both \
         decoders. A truncated encoding should almost always be rejected; if most are \
         accepted, the corpus is producing near-empty encodings and this test is not \
         probing anything."
    );
    println!("  truncation: {prefixes} prefixes, {rejections} joint rejections");
}

// ===========================================================================
// Family 2 — byte substitutions at EVERY offset
// ===========================================================================

/// `bool` bytes flipped to 2, `Option` tags pushed to 2, and a general sweep.
///
/// The decoder cannot be asked "where are the bools" without re-implementing
/// itself, so the mutation is applied at every offset. `2` is the smallest
/// value that is invalid for both a `bool` and an `Option` tag, so a single
/// sweep covers `InvalidBoolEncoding` and `InvalidTagEncoding` at once — and
/// the two must still be *distinguished* correctly, because a byte that is a
/// valid `Option` tag position and an invalid `bool` position (or vice versa)
/// changes which error fires and, more importantly, whether one fires at all.
#[test]
fn byte_substitution_at_every_offset_agrees() {
    let _serial = malformed_family_guard();
    let (mutations, rejections) = run_mutation_batches(MutationFamily::Byte);
    assert!(
        mutations > 100_000,
        "ANTI-VACUITY: only {mutations} substitutions were tried"
    );
    assert!(
        rejections > 0,
        "ANTI-VACUITY: no substitution was rejected by either decoder"
    );
    println!("  substitution: {mutations} mutants, {rejections} joint rejections");
}

// ===========================================================================
// Family 3 — variant indices pushed out of range
// ===========================================================================

/// Every 4-byte window is overwritten with a value just past each oneof's
/// variant count, and with `u32::MAX`.
///
/// `36`, `9` and `2` are the first INVALID index for `ExprInstance`,
/// `ConnectiveInstance` and `TaggedCont` respectively — the off-by-one that a
/// range check written with `>` instead of `>=` would let through, which would
/// then be a decoder that accepts a term the derived decoder refuses.
#[test]
fn out_of_range_variant_indices_agree() {
    let _serial = malformed_family_guard();
    let (mutations, rejections) = run_mutation_batches(MutationFamily::Variant);
    assert!(
        mutations > 100_000,
        "ANTI-VACUITY: only {mutations} variant-index mutations were tried"
    );
    assert!(
        rejections > 0,
        "ANTI-VACUITY: no variant mutation was rejected"
    );
    println!("  variant index: {mutations} mutants, {rejections} joint rejections");
}

// ===========================================================================
// Family 4 — hostile lengths
// ===========================================================================

/// Every 8-byte window is overwritten with `usize::MAX`, `u64::MAX` and other
/// enormous counts.
///
/// ⚠ **This is the family that catches an unsafe allocation attempt.** Without
/// serde's `size_hint::cautious` cap (reproduced by `bincode_decoder`'s
/// `cautious_capacity`, and made unnecessary for `Par`-bearing sequences by the
/// counted-repeat design), a length near `usize::MAX` could make one side try
/// to reserve unbounded memory while the other returns `Err` — a divergence
/// that no amount of valid-input testing can reach. The per-decode allocation
/// gate makes that regression fail with the exact mutation, and the child
/// batches prevent the oracle allocator's freed arenas accumulating in RSS.
#[test]
fn hostile_lengths_agree() {
    let _serial = malformed_family_guard();
    let (mutations, rejections) = run_mutation_batches(MutationFamily::Length);
    assert!(
        mutations > 100_000,
        "ANTI-VACUITY: only {mutations} length mutations were tried"
    );
    assert!(
        rejections > 0,
        "ANTI-VACUITY: no length mutation was rejected"
    );
    println!("  hostile lengths: {mutations} mutants, {rejections} joint rejections");
}

// ===========================================================================
// Family 5 — trailing bytes must be ACCEPTED
// ===========================================================================

/// `allow_trailing_bytes()` is part of `bincode::deserialize`'s configuration
/// (`bincode/src/lib.rs:177-185`). Rejecting trailing bytes would NARROW the
/// accepted language — a fork in the other direction, and an easy mistake for a
/// hand-written parser that "helpfully" checks it consumed everything.
#[test]
fn trailing_bytes_are_accepted_by_both() {
    let _serial = malformed_family_guard();
    let mut rejections = 0usize;
    let mut cases = 0usize;
    for (label, kind, bytes) in encodings() {
        for suffix in [vec![0u8], vec![0xFF; 16], vec![
            0x01, 0x02, 0x03, 0x04, 0x05,
        ]] {
            let mut mutant = bytes.clone();
            mutant.extend_from_slice(&suffix);
            cases += 1;
            check(
                || format!("{label} + {} trailing bytes", suffix.len()),
                kind,
                &mutant,
                &mut rejections,
            );
        }
    }
    assert!(
        cases > 100,
        "ANTI-VACUITY: only {cases} trailing-byte cases"
    );
    assert_eq!(
        rejections, 0,
        "trailing bytes must be ACCEPTED by both decoders; {rejections} case(s) were rejected"
    );
}

// ===========================================================================
// Family 6 — arbitrary short byte strings
// ===========================================================================

/// Byte strings that were never an encoding of anything. Most are rejected;
/// a few short ones legitimately decode to an empty value, and both decoders
/// must make the same call.
#[test]
fn arbitrary_byte_strings_agree() {
    let _serial = malformed_family_guard();
    let mut rejections = 0usize;
    let mut cases = 0usize;
    let mut seed: u64 = 0x2026_07_27;
    for len in 0..80usize {
        for _ in 0..40 {
            // xorshift64* — deterministic, so a failure is reproducible.
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let bytes: Vec<u8> = (0..len)
                .map(|i| ((seed >> ((i % 8) * 8)) as u8) ^ (i as u8))
                .collect();
            cases += 1;
            check(
                || format!("arbitrary[{len}] seed {seed:#x}"),
                Kind::Par,
                &bytes,
                &mut rejections,
            );
            check(
                || format!("arbitrary[{len}] seed {seed:#x} (TaggedContinuation)"),
                Kind::TaggedContinuation,
                &bytes,
                &mut rejections,
            );
        }
    }
    assert!(
        cases > 1_000,
        "ANTI-VACUITY: only {cases} arbitrary strings"
    );
    assert!(
        rejections > cases,
        "ANTI-VACUITY: {rejections} joint rejections over {cases} strings x 2 types — \
         random byte strings should almost all be rejected"
    );
    println!("  arbitrary: {cases} strings, {rejections} joint rejections");
}

// ===========================================================================
// Family 7 — the machine must never report a MachineInvariant
// ===========================================================================

/// `ColdStoreDecodeError::MachineInvariant` is documented as unreachable by
/// construction: it means a `*Build` op found its own `*Start`'s value missing.
/// It is an error rather than a panic so a hypothetical defect degrades one
/// read instead of aborting the node — but it must never actually fire, and
/// "never" is only a claim until something checks it against hostile input.
#[test]
fn no_input_reaches_a_machine_invariant() {
    let _serial = malformed_family_guard();
    use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecodeError;

    let mut checked = 0usize;
    for (label, _kind, bytes) in encodings() {
        for cut in 0..bytes.len() {
            checked += 1;
            if let Err(ColdStoreDecodeError::MachineInvariant(what)) =
                Par::cold_decode(&bytes[..cut])
            {
                panic!(
                    "{label} truncated at {cut} reached MachineInvariant({what}) — the \
                     opcode program's push/take pairing is wrong, which is a DECODER \
                     defect, not a bad input"
                );
            }
        }
    }
    assert!(
        checked > 50_000,
        "ANTI-VACUITY: only {checked} inputs checked"
    );
}
