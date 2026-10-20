//! # DR-131 P2b: the malformed-input differential, which pins the rejection set
//!
//! Ported from `feature/f1r3lang-mettail-only`
//! (`e5102e945:models/tests/bincode_decoder_malformed.rs`) to this branch's
//! machine, `models::rust::rholang::record_decoder`.
//!
//! ## Why this file matters most
//!
//! A decoder has two halves: which inputs it decodes correctly, and which
//! inputs it refuses. The second half decides whether the network stays
//! converged:
//!
//! > A node that accepts a byte string that another node rejects forks.
//!
//! Stored rows can come from peers, so "well-formed" is not an assumption
//! that the decoder can make. If the machine accepted a truncated row that the
//! derived decoder rejected, or the opposite, two nodes would disagree about
//! the contents of the same trie leaf.
//!
//! ## What is asserted
//!
//! The `Ok` or `Err` disposition agrees, and on `Ok` the values agree on every
//! field (`PartialEq` and the `Debug` text, because `PartialEq` skips
//! `locally_free`). The error messages are not compared: they belong to a
//! third-party crate. A machine error of the kind `MachineInvariant` is never
//! a valid rejection: it is a decoder defect, and every family fails on it.
//!
//! ## The mutation families
//!
//! Each family targets one thing that the derived decoder rejects, and runs on
//! every encoding of the corpus:
//!
//! ```text
//!   family                    what it probes
//!   ───────────────────────── ─────────────────────────────────────────────
//!   truncate at every offset  the end of input at every position: the only
//!                             family that cannot miss a resume point
//!   byte substitution         an invalid bool, an invalid Option tag, and a
//!                             broad sweep of other byte values
//!   variant index -> N        the first index past each oneof's arm count,
//!                             and u32::MAX
//!   u64 length -> huge        the allocation hazard: without a capped
//!                             preallocation, one side would abort on memory
//!                             and the other would return an error
//!   trailing bytes            both decoders must ACCEPT them
//!   arbitrary byte strings    inputs that were never an encoding
//!   machine invariant         no truncation reaches MachineInvariant
//! ```
//!
//! The substitution, variant and length families cannot know where a bool, a
//! tag or a length sits without a second parser. So they apply the mutation
//! at every byte offset. A mutation that lands on an unrelated byte must still
//! give the same disposition on both sides, which is the property under test.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fmt::Debug;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Mutex, MutexGuard};

use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
use models::rust::rholang::record_decoder::{
    CONNECTIVE_INSTANCE_VARIANT_COUNT, COST_VALUE_VARIANT_COUNT, EXPR_INSTANCE_VARIANT_COUNT,
    TAGGED_CONT_VARIANT_COUNT, UNF_INSTANCE_VARIANT_COUNT, VAR_INSTANCE_VARIANT_COUNT,
};
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError,
};
use serde::de::DeserializeOwned;

mod record_corpus;
use record_corpus as corpus;

/// The largest single allocation request since the last observation. The
/// allocation gate turns an allocator blow-up into a failure that names the
/// exact mutation and decoder side. Requests are tracked, not resident
/// memory, so allocator retention adds no noise.
static MAX_ALLOCATION_REQUEST: AtomicUsize = AtomicUsize::new(0);
static LIVE_ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_ALLOCATED_BYTES: AtomicUsize = AtomicUsize::new(0);

struct TrackingSystem;

#[global_allocator]
static TEST_ALLOCATOR: TrackingSystem = TrackingSystem;

unsafe impl GlobalAlloc for TrackingSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        MAX_ALLOCATION_REQUEST.fetch_max(layout.size(), AtomicOrdering::Relaxed);
        // SAFETY: this allocator only observes the `System` allocator.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record_live_growth(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        MAX_ALLOCATION_REQUEST.fetch_max(layout.size(), AtomicOrdering::Relaxed);
        // SAFETY: this allocator only observes the `System` allocator.
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record_live_growth(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` came from the `System` allocator.
        unsafe { System.dealloc(ptr, layout) }
        LIVE_ALLOCATED_BYTES.fetch_sub(layout.size(), AtomicOrdering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        MAX_ALLOCATION_REQUEST.fetch_max(new_size, AtomicOrdering::Relaxed);
        // SAFETY: `ptr` and `layout` came from the `System` allocator.
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

/// The one assertion of this file.
///
/// `rejections` counts the mutations that both sides rejected, so that a
/// family that stopped perturbing anything fails in its caller.
fn assert_agree<T, F>(label: F, bytes: &[u8], rejections: &mut usize)
where
    T: ColdStoreDecode + DeserializeOwned + PartialEq + Debug,
    F: FnOnce() -> String,
{
    // The machine gets the strict bound. The derived oracle can hold one
    // capped 1 MiB buffer for each nested sequence while it descends, so it
    // gets a wider bound.
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
            "{label}: a malformed-input decode exceeded its heap gate (oracle 64 MiB, machine \
             8 MiB; largest request: derived={oracle_max}, machine={machine_max}; peak live \
             delta: derived={oracle_peak}, machine={machine_peak}; input={} bytes)",
            bytes.len()
        );
    }
    match (oracle, machine) {
        (Ok(a), Ok(b)) => assert!(
            a == b && format!("{a:?}") == format!("{b:?}"),
            "{}: BOTH accepted, but produced DIFFERENT values. The machine and the derived \
             decoder must recognise the same language.",
            label()
        ),
        (Err(_), Err(ColdStoreDecodeError::MachineInvariant(what))) => panic!(
            "{}: the machine reached MachineInvariant({what}). Its push and take pairing is \
             wrong: a DECODER defect, not a bad input",
            label()
        ),
        (Err(_), Err(_)) => *rejections += 1,
        (Ok(a), Err(e)) => {
            let label = label();
            panic!(
                "{label}: the derived decoder ACCEPTED this byte string and the machine \
                 REJECTED it ({e}). A node that runs the machine would refuse state that its \
                 peers accept.\n  value: {a:?}"
            )
        }
        (Err(_), Ok(b)) => {
            let label = label();
            panic!(
                "{label}: the machine ACCEPTED a byte string that the derived decoder REJECTED. \
                 A node that runs the machine would admit state that its peers refuse.\n  \
                 value: {b:?}"
            )
        }
    }
}

/// The seven families are heavy and each walks the same corpus. Under
/// libtest they would run at the same time in one process and multiply their
/// allocator peaks. The guard serializes them in one process. Under nextest
/// each test is its own process, and the guard changes nothing. A poisoned
/// guard is recovered, so one failure does not hide the other families.
static MALFORMED_FAMILY: Mutex<()> = Mutex::new(());

fn malformed_family_guard() -> MutexGuard<'static, ()> {
    MALFORMED_FAMILY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Every corpus encoding, as `(label, root kind, bytes)`, for the four roots.
fn encodings() -> Vec<(String, Kind, Vec<u8>)> {
    let mut out: Vec<(String, Kind, Vec<u8>)> = Vec::with_capacity(128);
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

/// The out-of-range variant indices: the first invalid index of each oneof,
/// and `u32::MAX`. An off-by-one range check (`>` instead of `>=`) would let
/// the first invalid index through, and the machine would accept a term that
/// the derived decoder refuses. `COST_VALUE_VARIANT_COUNT` equals
/// `UNF_INSTANCE_VARIANT_COUNT` (6), so one entry covers both.
const OUT_OF_RANGE_INDICES: [u32; 6] = [
    TAGGED_CONT_VARIANT_COUNT,
    VAR_INSTANCE_VARIANT_COUNT,
    UNF_INSTANCE_VARIANT_COUNT,
    CONNECTIVE_INSTANCE_VARIANT_COUNT,
    EXPR_INSTANCE_VARIANT_COUNT,
    u32::MAX,
];

/// The heavy mutation sweeps run in bounded batches in child processes. A
/// child returns all its allocator arenas to the operating system when it
/// ends, so hundreds of thousands of hostile decodes do not accumulate
/// allocator retention in one process. Every child inherits the parent's
/// cgroup, and the 8 MiB gate checks each decode.
///
/// Offsets per child. One offset expands to at most six mutations, and each
/// mutation runs both decoders, so a child runs at most 24 mutations.
const MUTATION_BATCH_OFFSETS: usize = 4;
/// Children that run at the same time. The parent starts the next window only
/// after both children of a window end. Results are folded in source order,
/// so scheduling cannot change counts or diagnostics.
const MUTATION_BATCH_WORKERS: usize = 2;
const MUTATION_CHILD_FAMILY: &str = "RECORD_DECODER_MALFORMED_CHILD_FAMILY";
const MUTATION_CHILD_FIXTURE: &str = "RECORD_DECODER_MALFORMED_CHILD_FIXTURE";
const MUTATION_CHILD_START: &str = "RECORD_DECODER_MALFORMED_CHILD_START";
const MUTATION_CHILD_END: &str = "RECORD_DECODER_MALFORMED_CHILD_END";
const MUTATION_BATCH_RESULT: &str = "RECORD_DECODER_MALFORMED_BATCH_RESULT";

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
    let executable = std::env::current_exe().expect("locate the malformed-input test executable");
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
                    .map(|handle| handle.join().expect("a malformed mutation worker panicked"))
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
                "failed to start the {} mutation child for {label}[{start}..{end}]: {error}",
                family.name()
            )
        });
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the {} mutation child failed for {label}[{start}..{end}] with {}\nstdout:\n{}\n\
         stderr:\n{}",
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
                "the {} mutation child for {label}[{start}..{end}] returned no result \
                 marker\nstdout:\n{stdout}\nstderr:\n{stderr}",
                family.name()
            )
        });
    let mut fields = line.split_whitespace();
    assert_eq!(fields.next(), Some(MUTATION_BATCH_RESULT));
    let mutations = fields
        .next()
        .expect("the batch result has a mutation count")
        .parse::<usize>()
        .expect("the batch mutation count is numeric");
    let rejections = fields
        .next()
        .expect("the batch result has a rejection count")
        .parse::<usize>()
        .expect("the batch rejection count is numeric");
    assert!(
        fields.next().is_none(),
        "unexpected fields in the mutation batch result {line:?}"
    );
    (mutations, rejections)
}

fn required_child_usize(name: &str) -> usize {
    std::env::var(name)
        .unwrap_or_else(|_| panic!("the mutation child requires {name}"))
        .parse()
        .unwrap_or_else(|error| panic!("the mutation child's {name} is not a usize: {error}"))
}

/// One bounded batch, run by [`run_mutation_batches`]. It is ignored, so a
/// normal run never counts a worker as an independent test.
#[test]
#[ignore = "child process of the malformed-input allocation gate"]
fn mutation_batch_child() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
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
        .unwrap_or_else(|| panic!("the mutation child's fixture {fixture} is out of range"));
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
                for index in OUT_OF_RANGE_INDICES {
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

#[test]
fn the_out_of_range_indices_are_the_first_invalid_index_of_each_oneof() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    assert_eq!(COST_VALUE_VARIANT_COUNT, UNF_INSTANCE_VARIANT_COUNT);
    assert_eq!(
        OUT_OF_RANGE_INDICES,
        [2, 3, 6, 9, 42, u32::MAX],
        "the arm counts of this branch's oneofs"
    );
}

// ===========================================================================
// Family 1: truncation at every byte offset
// ===========================================================================

/// The exhaustive family. Every prefix of every corpus encoding goes to both
/// decoders. It is the only mutation that visits every resume point of the
/// machine: a `*Build` op that read its scalars in the wrong order, or a
/// resume point that read one byte too many, changes where the input runs
/// out, and so changes the disposition of some prefix.
///
/// The cost is quadratic in the encoding length, so the deep cases stay at 48
/// levels.
#[test]
fn truncation_at_every_offset_agrees() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
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
         decoders. A truncated encoding is almost always invalid, so the corpus must be \
         producing near-empty encodings."
    );
    println!("  truncation: {prefixes} prefixes, {rejections} joint rejections");
}

// ===========================================================================
// Family 2: byte substitution at every offset
// ===========================================================================

/// `2` is the smallest value that is invalid for both a `bool` and an
/// `Option` tag, so one sweep covers both errors. The two must still be told
/// apart correctly, because a byte that is a valid tag and an invalid bool, or
/// the opposite, changes whether an error fires at all.
#[test]
fn byte_substitution_at_every_offset_agrees() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
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
// Family 3: variant indices out of range
// ===========================================================================

/// Every 4-byte window is overwritten with each of [`OUT_OF_RANGE_INDICES`].
#[test]
fn out_of_range_variant_indices_agree() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
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
// Family 4: hostile lengths
// ===========================================================================

/// Every 8-byte window is overwritten with `usize::MAX`, `u64::MAX` and other
/// very large counts.
///
/// This family catches an unsafe allocation. Without a capped preallocation
/// (serde's `size_hint::cautious`, which the machine's leaf readers copy as
/// `cautious_capacity`, and the counted repeat for `Par`-bearing sequences), a
/// length near `usize::MAX` could make one side reserve unbounded memory while
/// the other returns an error. No valid-input test can reach that divergence.
#[test]
fn hostile_lengths_agree() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
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
// Family 5: trailing bytes must be ACCEPTED
// ===========================================================================

/// `allow_trailing_bytes()` is part of the reader's options. A decoder that
/// checked that it consumed everything would accept a smaller language, which
/// is a fork in the other direction.
#[test]
fn trailing_bytes_are_accepted_by_both() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
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
// Family 6: arbitrary short byte strings
// ===========================================================================

/// Byte strings that were never an encoding. Most are rejected. A few short
/// ones decode to an empty value, and both decoders must decide the same way.
#[test]
fn arbitrary_byte_strings_agree() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let _serial = malformed_family_guard();
    let mut rejections = 0usize;
    let mut cases = 0usize;
    let mut seed: u64 = 0x2026_0727;
    for len in 0..80usize {
        for _ in 0..40 {
            // xorshift64: deterministic, so a failure is reproducible.
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let bytes: Vec<u8> = (0..len)
                .map(|i| ((seed >> ((i % 8) * 8)) as u8) ^ (i as u8))
                .collect();
            cases += 1;
            for kind in [
                Kind::Par,
                Kind::ListParWithRandom,
                Kind::BindPattern,
                Kind::TaggedContinuation,
            ] {
                check(
                    || format!("arbitrary[{len}] seed {seed:#x}"),
                    kind,
                    &bytes,
                    &mut rejections,
                );
            }
        }
    }
    assert!(
        cases > 1_000,
        "ANTI-VACUITY: only {cases} arbitrary strings"
    );
    assert!(
        rejections > 2 * cases,
        "ANTI-VACUITY: {rejections} joint rejections over {cases} strings and 4 roots. \
         Random byte strings should almost all be rejected"
    );
    println!("  arbitrary: {cases} strings, {rejections} joint rejections");
}

// ===========================================================================
// Family 7: the machine never reports MachineInvariant
// ===========================================================================

/// `MachineInvariant` means that a `*Build` op did not find the value of its
/// own `*Start`. It is an error and not a panic, so that a defect costs one
/// read and not the node. It must never fire, and that is only a claim until
/// hostile input checks it. `assert_agree` fails on it in every family. This
/// family also decodes every truncation as each of the four roots, not only
/// as the root of its encoding.
#[test]
fn no_input_reaches_a_machine_invariant() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    let _serial = malformed_family_guard();

    fn invariant<T: ColdStoreDecode>(bytes: &[u8]) -> Option<&'static str> {
        match T::cold_decode(bytes) {
            Err(ColdStoreDecodeError::MachineInvariant(what)) => Some(what),
            Ok(_) | Err(_) => None,
        }
    }

    let mut checked = 0usize;
    for (label, _kind, bytes) in encodings() {
        for cut in 0..bytes.len() {
            let prefix = &bytes[..cut];
            checked += 1;
            for (root, reached) in [
                ("Par", invariant::<Par>(prefix)),
                ("ListParWithRandom", invariant::<ListParWithRandom>(prefix)),
                ("BindPattern", invariant::<BindPattern>(prefix)),
                (
                    "TaggedContinuation",
                    invariant::<TaggedContinuation>(prefix),
                ),
            ] {
                if let Some(what) = reached {
                    panic!(
                        "{label} truncated at {cut} and decoded as {root} reached \
                         MachineInvariant({what}): the opcode program's push and take pairing \
                         is wrong, a DECODER defect"
                    );
                }
            }
        }
    }
    assert!(
        checked > 50_000,
        "ANTI-VACUITY: only {checked} inputs checked"
    );
}
