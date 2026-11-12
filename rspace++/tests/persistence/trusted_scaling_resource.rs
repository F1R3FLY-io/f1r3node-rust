use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::mem::MaybeUninit;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, mpsc};
use std::time::Instant;

use shared::rust::test_scratch;

use super::checkpoint_real_io_resource::{LinuxIo, linux_filesystem_magic, status_bytes};
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{catalog_image, limit_config};
use super::kind1_recovery_protocol_properties::{AllocationMeasurement, measure_allocations};
use super::retention_v2_editor_properties::{METADATA_STEP_WORK_BOUND, PATH_LOCAL_BYTE_BOUND};
use super::support::TestSemanticProfile;
use super::trusted_scaling_properties::{
    CommitProbe, CommitRecord, Phase, ScalingChain, ScalingFixture, admit_scaling_directory,
    mutation_batch, scaling_key,
};
use super::trusted_successor_production::{PublicationEffects, RECLAMATION_INTENT};
use crate::rspace::persistence::durable::{
    DurableAdmissionLimitsV1, DurableAuthorityCoreV1, DurableDirectoryV1, DurableWriterLeaseV1,
    SemanticallyAdmittedDurableWriterV1, TrustedSuccessorPolicyV1, acquire_cold_checked,
    index_file_name, pack_file_name, publish_trusted_incremental_v1,
};
use crate::rspace::persistence::retention::v2_format::RetentionFormat;
use crate::rspace::persistence::{
    CatalogSlotV1, DagTransactionLimits, DurableLimitConfig, DurableLimits, GenerationLeaseKeyV1,
    GenerationLeaseV1, INDEX_RECORD_LEN_V1, IndexGenerationV1, LogPackDecodeLimits,
    MutationBatchViewV1, ObjectDigest, OpenDagTransaction, PackId,
};

type Core = DurableAuthorityCoreV1<TestSemanticProfile>;

const CELL_ENV: &str = "RSPACE_SCALE_CELL";
const MODE_ENV: &str = "RSPACE_SCALE_MODE";
const FIXTURE_ENV: &str = "RSPACE_SCALE_FIXTURE";
const SCRATCH_ENV: &str = "RSPACE_SCALE_SCRATCH";
const CHILD_TEST: &str = "rspace::persistence::tests::trusted_scaling_resource::scale_child";
const CHILD_ARGUMENTS: [&str; 6] =
    ["--exact", CHILD_TEST, "--ignored", "--nocapture", "--quiet", "--test-threads=1"];
const RECORD_PREFIX: &str = "SCALE_";
const RECORD_VERSION: &str = "v1";
const HEADER: &str = "SCALE_HEADER";
const FIXTURE: &str = "SCALE_FIXTURE";
const VERIFY: &str = "SCALE_VERIFY";
const SAMPLE: &str = "SCALE_SAMPLE";
const PROCESS: &str = "SCALE_PROCESS";
const CATALOG: &str = "catalog.v1";
const WORKER_STACK_BYTES: usize = 8 * 1024 * 1024;
const IO_BUFFER_BYTES: usize = 512;
const STATUS_BUFFER_BYTES: usize = 8 * 1024;
const TMPFS_MAGIC: u64 = 0x0102_1994;
const RAMFS_MAGIC: u64 = 0x8584_58f6;
const EXT4_MAGIC: u64 = 0xef53;
const XFS_MAGIC: u64 = 0x5846_5342;
const BTRFS_MAGIC: u64 = 0x9123_683e;
const SPLITMIX_GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;
const SPLITMIX_FIRST_MULTIPLIER: u64 = 0xbf58_476d_1ce4_e5b9;
const SPLITMIX_SECOND_MULTIPLIER: u64 = 0x94d0_49bb_1331_11eb;
const RECORD_HEADROOM_BYTES: u64 = 1 << 23;
const MUTATION_BYTES_PER_KEY: u64 = 4096;
const COMMIT_RECORD_SLACK: u64 = METADATA_STEP_WORK_BOUND + 1;
const PHASES: [Phase; 4] = [Phase::Seal, Phase::Bind, Phase::Prepare, Phase::Publish];
const PROBED_PHASE_PREFIXES: [&str; 4] = ["seal_", "bind_", "prepare_", "publish_"];
const CELL_KEYS: [&str; 12] = [
    "axis", "keys", "reserve", "history", "delta", "deletion", "writer", "case", "warmup",
    "deferred", "timed", "seed",
];
const HEADER_KEYS: [&str; 2] = ["mode", "cell"];
const FIXTURE_KEYS: [&str; 9] = [
    "keys_total",
    "state_objects",
    "state_bytes",
    "history",
    "union_objects",
    "pack_bytes",
    "index_bytes",
    "records",
    "build_ns",
];
const VERIFY_KEYS: [&str; 59] = [
    "commit",
    "label",
    "target",
    "cur_refs",
    "cur_exp",
    "cur_quoted",
    "ret_refs",
    "ret_exp",
    "ret_quoted",
    "meta_refs",
    "meta_exp",
    "meta_quoted",
    "logical_writes",
    "metadata_bytes",
    "fresh_objects",
    "fresh_bytes",
    "pack_delta",
    "index_delta",
    "live_packs",
    "old_path",
    "new_path",
    "deleted_objects",
    "loaded_nodes",
    "edited_nodes",
    "emitted_nodes",
    "exact_path_chunks",
    "seal_alloc_count",
    "seal_alloc_bytes",
    "seal_alloc_peak",
    "seal_alloc_max",
    "seal_syscr",
    "seal_syscw",
    "seal_rchar",
    "seal_wchar",
    "bind_alloc_count",
    "bind_alloc_bytes",
    "bind_alloc_peak",
    "bind_alloc_max",
    "bind_syscr",
    "bind_syscw",
    "bind_rchar",
    "bind_wchar",
    "prepare_alloc_count",
    "prepare_alloc_bytes",
    "prepare_alloc_peak",
    "prepare_alloc_max",
    "prepare_syscr",
    "prepare_syscw",
    "prepare_rchar",
    "prepare_wchar",
    "publish_alloc_count",
    "publish_alloc_bytes",
    "publish_alloc_peak",
    "publish_alloc_max",
    "publish_syscr",
    "publish_syscw",
    "publish_rchar",
    "publish_wchar",
    "unlinked",
];
const SAMPLE_KEYS: [&str; 19] = [
    "commit",
    "label",
    "target",
    "seal_thread_ns",
    "seal_process_ns",
    "seal_wall_ns",
    "bind_thread_ns",
    "bind_process_ns",
    "bind_wall_ns",
    "prepare_thread_ns",
    "prepare_process_ns",
    "prepare_wall_ns",
    "publish_thread_ns",
    "publish_process_ns",
    "publish_wall_ns",
    "release_thread_ns",
    "release_process_ns",
    "release_wall_ns",
    "rss_after",
];
const PROCESS_KEYS: [&str; 16] = [
    "mode",
    "commits",
    "admission_ns",
    "rss_start",
    "rss_end",
    "hwm_window",
    "hwm_reset",
    "swap",
    "live_packs",
    "filesystem",
    "probe_syscr",
    "probe_syscw",
    "probe_rchar",
    "probe_wchar",
    "probe_alloc_count",
    "probe_alloc_bytes",
];
const PROBE_KEYS: [&str; 6] = [
    "probe_syscr",
    "probe_syscw",
    "probe_rchar",
    "probe_wchar",
    "probe_alloc_count",
    "probe_alloc_bytes",
];
const HARNESS_CELLS: [&str; 3] = [
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=E,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=evict,warmup=2,\
     deferred=2,timed=2,seed=7929",
    "axis=L,keys=16,reserve=16,history=16,delta=0,deletion=2,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
];
const MALFORMED_CELLS: [&str; 27] = [
    "",
    "axis=U",
    "keys=16,axis=U,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929,extra=1",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929,",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929 ",
    "axis=X,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=016,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=+16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16=1,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=0,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16,reserve=0,history=0,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=0,seed=7929",
    "axis=U,keys=16,reserve=0,history=16,delta=0,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16,reserve=16,history=16,delta=1,deletion=2,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v3,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=other,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=1,timed=4,seed=7929",
    "axis=E,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=evict,warmup=2,\
     deferred=0,timed=2,seed=7929",
    "axis=U,keys=4,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16,reserve=2,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=L,keys=16,reserve=15,history=16,delta=0,deletion=2,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=L,keys=16,reserve=4,history=16,delta=0,deletion=2,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=7929",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,warmup=2,\
     deferred=0,timed=4,seed=18446744073709551616",
    "axis=U,keys=16,reserve=0,history=16,delta=1,deletion=0,writer=v2,case=ordinary,\
     warmup=18446744073709551615,deferred=0,timed=4,seed=7929",
];
const IO_SAMPLE: &str = "rchar: 4096\nwchar: 2048\nsyscr: 12\nsyscw: 6\nread_bytes: \
                         512\nwrite_bytes: 1024\ncancelled_write_bytes: 0\n";
const IO_FIELDS: [&str; 6] =
    ["rchar:", "wchar:", "syscr:", "syscw:", "read_bytes:", "write_bytes:"];
const STATUS_SAMPLE: &str =
    "Name:\tscale\nVmHWM:\t    2048 kB\nVmRSS:\t    1024 kB\nVmSwap:\t       0 kB\n";
const STATUS_FIELDS: [&str; 3] = ["VmRSS:", "VmHWM:", "VmSwap:"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Build,
    Verify,
    Time,
}

impl Mode {
    const fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Verify => "verify",
            Self::Time => "time",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Ordinary,
    Evict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Label {
    Warmup,
    Deferred,
    Reclaim,
    Timed,
}

impl Label {
    const fn name(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Deferred => "deferred",
            Self::Reclaim => "reclaim",
            Self::Timed => "timed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScaleCell {
    keys: usize,
    reserve: usize,
    history: usize,
    delta: usize,
    deletion: usize,
    writer: RetentionFormat,
    case: Case,
    warmup: usize,
    deferred: usize,
    timed: usize,
    seed: u64,
    keys_total: usize,
    commits: usize,
}

impl ScaleCell {
    fn label(&self, commit: usize) -> Label {
        match (self.case, commit.checked_sub(self.warmup)) {
            (_, None) => Label::Warmup,
            (Case::Ordinary, Some(_)) => Label::Timed,
            (Case::Evict, Some(after)) => match after.cmp(&self.deferred) {
                Ordering::Less => Label::Deferred,
                Ordering::Equal => Label::Reclaim,
                Ordering::Greater => Label::Timed,
            },
        }
    }
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn draw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(SPLITMIX_GAMMA);
        let mixed = (self.0 ^ (self.0 >> 30)).wrapping_mul(SPLITMIX_FIRST_MULTIPLIER);
        let mixed = (mixed ^ (mixed >> 27)).wrapping_mul(SPLITMIX_SECOND_MULTIPLIER);
        mixed ^ (mixed >> 31)
    }
}

#[derive(Clone, Copy, Debug)]
struct Permutation {
    offset: u64,
    stride: u64,
    length: u64,
}

impl Permutation {
    fn new(seed: u64, length: usize) -> Self {
        let length = u64::try_from(length).expect("a schedule length fits u64");
        assert!(length > 0, "a schedule permutes a nonempty range");
        let mut stream = SplitMix64(seed);
        let offset = stream.draw() % length;
        let mut stride = (stream.draw() % length) | 1;
        while greatest_common_divisor(stride, length) != 1 {
            stride = stride
                .checked_add(2)
                .expect("a coprime stride exists below the overflow bound");
        }
        Self {
            offset,
            stride,
            length,
        }
    }

    fn position(&self, index: usize) -> usize {
        let index = u128::from(u64::try_from(index).expect("a schedule index fits u64"));
        let position =
            (u128::from(self.offset) + index * u128::from(self.stride)) % u128::from(self.length);
        usize::try_from(position).expect("a schedule position fits usize")
    }
}

struct Schedule {
    cell: ScaleCell,
    permutation: Permutation,
}

impl Schedule {
    fn new(cell: ScaleCell) -> Self {
        let length = match cell.deletion {
            0 => cell.keys,
            deletion => cell.reserve / deletion,
        };
        Self {
            cell,
            permutation: Permutation::new(cell.seed, length),
        }
    }

    fn edits(&self, commit: usize) -> Vec<(usize, u64)> {
        match self.cell.deletion {
            0 => {
                let after = u64::try_from(commit)
                    .ok()
                    .and_then(|commit| commit.checked_add(2))
                    .expect("a replacement multiplicity fits u64");
                let first = commit * self.cell.delta;
                (first..first + self.cell.delta)
                    .map(|position| (self.permutation.position(position), after))
                    .collect()
            }
            deletion => {
                let first = self.cell.keys + self.permutation.position(commit) * deletion;
                (first..first + deletion).map(|index| (index, 0)).collect()
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Memory {
    rss: u64,
    hwm: u64,
    swap: u64,
}

#[derive(Clone, Copy, Debug)]
struct Reading {
    thread: u64,
    process: u64,
    wall: u64,
}

impl Reading {
    fn now(epoch: Instant) -> Self {
        Self {
            thread: clock_ns(libc::CLOCK_THREAD_CPUTIME_ID).expect("the thread CPU clock reads"),
            process: clock_ns(libc::CLOCK_PROCESS_CPUTIME_ID).expect("the process CPU clock reads"),
            wall: u64::try_from(epoch.elapsed().as_nanos()).expect("the wall clock fits u64"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PhaseCost {
    allocations: AllocationMeasurement,
    io: LinuxIo,
}

#[derive(Default)]
struct MeasuredProbe {
    costs: [Option<PhaseCost>; 4],
}

impl MeasuredProbe {
    fn take(&mut self) -> [PhaseCost; 4] {
        std::mem::take(&mut self.costs)
            .map(|cost| cost.expect("every phase of a verified commit is probed once"))
    }
}

impl CommitProbe for MeasuredProbe {
    fn phase<T>(&mut self, phase: Phase, operation: impl FnOnce() -> T) -> T {
        let before = read_io();
        let (result, allocations) = measure_allocations(operation);
        let after = read_io();
        let io = after
            .difference(before)
            .expect("the /proc/self/io counters never decrease");
        let previous = self.costs[phase_index(phase)].replace(PhaseCost { allocations, io });
        assert!(previous.is_none(), "the {phase:?} phase ran twice in one commit");
        result
    }
}

#[derive(Clone, Copy, Debug)]
struct Window {
    rss_start: u64,
    hwm_reset: bool,
}

#[derive(Clone, Copy, Debug)]
struct WindowReport {
    rss_start: u64,
    rss_end: u64,
    hwm_window: u64,
    hwm_reset: bool,
    swap: u64,
}

impl Window {
    fn open() -> Self {
        let cleared = OpenOptions::new()
            .write(true)
            .open("/proc/self/clear_refs")
            .and_then(|mut file| file.write_all(b"5"))
            .is_ok();
        let memory = read_memory();
        let page = u64::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) })
            .expect("the page size is positive");
        let reset_bound = memory
            .rss
            .checked_add(page)
            .expect("the resident set plus one page fits u64");
        Self {
            rss_start: memory.rss,
            hwm_reset: cleared && memory.hwm <= reset_bound,
        }
    }

    fn close(self) -> WindowReport {
        let memory = read_memory();
        WindowReport {
            rss_start: self.rss_start,
            rss_end: memory.rss,
            hwm_window: memory.hwm,
            hwm_reset: self.hwm_reset,
            swap: memory.swap,
        }
    }
}

struct ProcessRecord {
    mode: Mode,
    commits: usize,
    admission_ns: u64,
    window: WindowReport,
    live_packs: u64,
    filesystem: &'static str,
    probes: [u64; 6],
}

impl ProcessRecord {
    fn emit(&self) {
        let mut values = Vec::with_capacity(PROCESS_KEYS.len());
        values.push(self.mode.name().to_owned());
        decimal(&mut values, [
            wide(self.commits),
            self.admission_ns,
            self.window.rss_start,
            self.window.rss_end,
            self.window.hwm_window,
            u64::from(self.window.hwm_reset),
            self.window.swap,
            self.live_packs,
        ]);
        values.push(self.filesystem.to_owned());
        decimal(&mut values, self.probes);
        emit(PROCESS, &PROCESS_KEYS, &values);
    }
}

struct Sample {
    label: Label,
    target: ObjectDigest,
    readings: [Reading; 6],
    rss_after: u64,
}

struct TimedRun {
    admission_ns: u64,
    samples: Vec<Sample>,
    window: WindowReport,
    live_packs: u64,
}

struct Record {
    tag: String,
    keys: &'static [&'static str],
    values: Vec<String>,
}

impl Record {
    fn get(&self, key: &str) -> &str {
        let position = self
            .keys
            .iter()
            .position(|candidate| *candidate == key)
            .unwrap_or_else(|| panic!("{} records carry no {key}", self.tag));
        &self.values[position]
    }

    fn integer(&self, key: &str) -> u64 {
        let value = self.get(key);
        canonical(value)
            .unwrap_or_else(|| panic!("{} {key}={value} is not a canonical integer", self.tag))
    }
}

fn wide(value: usize) -> u64 { u64::try_from(value).expect("a harness count fits u64") }

fn greatest_common_divisor(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

fn canonical(value: &str) -> Option<u64> {
    match value.as_bytes() {
        [b'0'] => Some(0),
        [b'1'..=b'9', rest @ ..] if rest.iter().all(u8::is_ascii_digit) => value.parse().ok(),
        _ => None,
    }
}

fn parse_cell(text: &str) -> Option<ScaleCell> {
    let mut fields = text.split(',');
    let mut values = [""; CELL_KEYS.len()];
    for (value, key) in values.iter_mut().zip(CELL_KEYS) {
        let (name, field) = fields.next()?.split_once('=')?;
        if name != key {
            return None;
        }
        *value = field;
    }
    if fields.next().is_some() {
        return None;
    }
    let [
        axis,
        keys,
        reserve,
        history,
        delta,
        deletion,
        writer,
        case,
        warmup,
        deferred,
        timed,
        seed,
    ] = values;
    if !matches!(axis, "U" | "R" | "D" | "L" | "E") {
        return None;
    }
    let size = |value: &str| canonical(value).and_then(|value| usize::try_from(value).ok());
    let keys = size(keys)?;
    let reserve = size(reserve)?;
    let delta = size(delta)?;
    let deletion = size(deletion)?;
    let warmup = size(warmup)?;
    let deferred = size(deferred)?;
    let timed = size(timed)?;
    let case = match case {
        "ordinary" => Case::Ordinary,
        "evict" => Case::Evict,
        _ => return None,
    };
    let reclaim = match (case, deferred) {
        (Case::Ordinary, 0) => 0,
        (Case::Evict, 1..) => 1,
        _ => return None,
    };
    let commits = warmup
        .checked_add(deferred)?
        .checked_add(reclaim)?
        .checked_add(timed)?;
    let schedulable = match (delta, deletion) {
        (1.., 0) => reserve == 0 && commits.checked_mul(delta)? <= keys,
        (0, 1..) => reserve % deletion == 0 && commits <= reserve / deletion,
        _ => false,
    };
    let cell = ScaleCell {
        keys,
        reserve,
        history: size(history)?,
        delta,
        deletion,
        writer: match writer {
            "v2" => RetentionFormat::V2,
            "v1" => RetentionFormat::V1,
            _ => return None,
        },
        case,
        warmup,
        deferred,
        timed,
        seed: canonical(seed)?,
        keys_total: keys.checked_add(reserve)?,
        commits,
    };
    (schedulable && cell.keys >= 1 && cell.history >= 1 && cell.timed >= 1).then_some(cell)
}

fn read_proc<'buffer>(path: &str, buffer: &'buffer mut [u8]) -> Option<&'buffer str> {
    let mut file = File::open(path).ok()?;
    let mut filled = 0;
    loop {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => {
                filled += read;
                if filled == buffer.len() {
                    return None;
                }
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    std::str::from_utf8(&buffer[..filled]).ok()
}

fn read_io() -> LinuxIo {
    let mut buffer = [0_u8; IO_BUFFER_BYTES];
    read_proc("/proc/self/io", &mut buffer)
        .and_then(LinuxIo::parse)
        .expect("/proc/self/io reads whole and carries every counter")
}

fn parse_memory(status: &str) -> Option<Memory> {
    Some(Memory {
        rss: status_bytes(status, "VmRSS:")?,
        hwm: status_bytes(status, "VmHWM:")?,
        swap: status_bytes(status, "VmSwap:")?,
    })
}

fn read_memory() -> Memory {
    let mut buffer = [0_u8; STATUS_BUFFER_BYTES];
    read_proc("/proc/self/status", &mut buffer)
        .and_then(parse_memory)
        .expect("/proc/self/status reads whole and carries VmRSS, VmHWM and VmSwap")
}

fn clock_ns(clock: libc::clockid_t) -> Option<u64> {
    let mut time = MaybeUninit::<libc::timespec>::uninit();
    if unsafe { libc::clock_gettime(clock, time.as_mut_ptr()) } != 0 {
        return None;
    }
    let time = unsafe { time.assume_init() };
    u64::try_from(time.tv_sec)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(time.tv_nsec).ok()?)
}

fn positive(start: u64, end: u64) -> Option<u64> {
    end.checked_sub(start).filter(|delta| *delta > 0)
}

fn non_negative(start: u64, end: u64) -> Option<u64> { end.checked_sub(start) }

fn wall_ns(started: Instant) -> u64 {
    let elapsed = u64::try_from(started.elapsed().as_nanos()).expect("an elapsed time fits u64");
    positive(0, elapsed).expect("a timed routine takes a positive wall time")
}

fn phase_durations(readings: &[Reading; 6]) -> Option<[[u64; 3]; 5]> {
    let mut durations = [[0; 3]; 5];
    for (duration, pair) in durations.iter_mut().zip(readings.windows(2)) {
        *duration = [
            non_negative(pair[0].thread, pair[1].thread)?,
            non_negative(pair[0].process, pair[1].process)?,
            positive(pair[0].wall, pair[1].wall)?,
        ];
    }
    Some(durations)
}

fn counted(allocations: AllocationMeasurement) -> Option<AllocationMeasurement> {
    (allocations.count > 0).then_some(allocations)
}

const fn phase_index(phase: Phase) -> usize {
    match phase {
        Phase::Seal => 0,
        Phase::Bind => 1,
        Phase::Prepare => 2,
        Phase::Publish => 3,
    }
}

fn classify_filesystem(magic: u64) -> Option<&'static str> {
    match magic {
        TMPFS_MAGIC | RAMFS_MAGIC => None,
        EXT4_MAGIC => Some("ext4"),
        XFS_MAGIC => Some("xfs"),
        BTRFS_MAGIC => Some("btrfs"),
        _ => Some("other"),
    }
}

fn classify_directory(path: &Path) -> &'static str {
    classify_filesystem(linux_filesystem_magic(path))
        .unwrap_or_else(|| panic!("{} is on tmpfs or ramfs", path.display()))
}

fn require_empty(path: &Path) {
    let mut entries = fs::read_dir(path).expect("a harness directory is readable");
    assert!(entries.next().is_none(), "{} is not empty", path.display());
}

fn directory_names(path: &Path) -> BTreeSet<String> {
    fs::read_dir(path)
        .expect("a harness directory is readable")
        .map(|entry| {
            entry
                .expect("a directory entry is readable")
                .file_name()
                .into_string()
                .expect("a durable file name is UTF-8")
        })
        .collect()
}

fn fixture_file_names() -> [String; 3] {
    [CATALOG.to_owned(), pack_file_name(PackId::GENESIS), index_file_name(IndexGenerationV1::FIRST)]
}

fn require_fixture_files(directory: &Path, fixture: &ScalingFixture) {
    let names = fixture_file_names();
    assert_eq!(
        directory_names(directory),
        names.iter().cloned().collect::<BTreeSet<_>>(),
        "the fixture directory holds exactly the checkpoint image"
    );
    let [catalog, pack, index] =
        names.map(|name| fs::read(directory.join(name)).expect("a fixture file is readable"));
    assert!(
        catalog == catalog_image(fixture.image.catalog),
        "the fixture catalog differs from the rebuilt fixture"
    );
    assert!(pack == *fixture.image.pack, "the fixture pack differs from the rebuilt fixture");
    assert!(
        index == fixture.image.canonical_index,
        "the fixture index differs from the rebuilt fixture"
    );
}

fn copy_fixture(fixture: &Path, scratch: &Path) -> (u64, u64) {
    let names = fixture_file_names();
    assert_eq!(
        directory_names(fixture),
        names.iter().cloned().collect::<BTreeSet<_>>(),
        "the fixture directory holds exactly the checkpoint image"
    );
    let [_, pack, index] = names.map(|name| {
        let mut source = File::open(fixture.join(&name)).expect("open a fixture file");
        let mut target = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(scratch.join(&name))
            .expect("create the scratch copy of a fixture file");
        let length = io::copy(&mut source, &mut target).expect("copy a fixture file");
        target
            .sync_all()
            .expect("sync the scratch copy of a fixture file");
        length
    });
    File::open(scratch)
        .and_then(|directory| directory.sync_all())
        .expect("sync the scratch directory");
    (pack, index)
}

fn scale_limits(cell: &ScaleCell, pack_bytes: u64, index_bytes: u64) -> DurableLimits {
    let base = limit_config();
    let bounded = |value: Option<u64>| value.expect("the scale limits fit u64");
    let records = index_bytes / INDEX_RECORD_LEN_V1;
    let commits = bounded(wide(cell.commits).checked_add(1));
    let edited = wide(cell.delta.max(cell.deletion));
    let commit_records = bounded(records.checked_add(COMMIT_RECORD_SLACK));
    let commit_bytes = bounded(
        pack_bytes
            .checked_mul(4)
            .and_then(|bytes| bytes.checked_add(PATH_LOCAL_BYTE_BOUND))
            .and_then(|bytes| bytes.checked_add(RECORD_HEADROOM_BYTES))
            .and_then(|bytes| bytes.checked_add(MUTATION_BYTES_PER_KEY.checked_mul(edited)?)),
    );
    let appended_records = bounded(commits.checked_mul(commit_records));
    let total_records = bounded(records.checked_add(appended_records));
    let total_index = bounded(
        appended_records
            .checked_mul(INDEX_RECORD_LEN_V1)
            .and_then(|bytes| bytes.checked_add(index_bytes)),
    );
    let total_pack = bounded(
        commits
            .checked_mul(commit_bytes)
            .and_then(|bytes| bytes.checked_add(pack_bytes)),
    );
    let reads = bounded(total_pack.checked_add(total_index));
    DurableLimits::new(DurableLimitConfig {
        max_live_packs: base.max_live_packs,
        max_pack_bytes: total_pack.max(base.max_pack_bytes),
        max_transactions: bounded(commits.checked_add(1)).max(base.max_transactions),
        max_object_records: total_records.max(base.max_object_records),
        max_total_scan_bytes: reads.max(base.max_total_scan_bytes),
        max_total_physical_read_bytes: reads.max(base.max_total_physical_read_bytes),
        max_index_bytes: total_index.max(base.max_index_bytes),
        max_index_records: total_records.max(base.max_index_records),
        max_full_byte_comparison_bytes: total_pack.max(base.max_full_byte_comparison_bytes),
        max_aggregate_full_byte_comparison_bytes: total_pack
            .max(base.max_aggregate_full_byte_comparison_bytes),
    })
    .expect("the scale limits are nonzero")
}

fn probe_io_cost() -> LinuxIo {
    let first = read_io();
    let second = read_io();
    let cost = second
        .difference(first)
        .expect("the /proc/self/io counters never decrease");
    assert!(cost.syscr > 0, "one /proc/self/io read counts no read call");
    cost
}

fn probe_trace_allocations() -> AllocationMeasurement {
    let points = PublicationEffects::trusted().trace;
    let (trace, allocations) = measure_allocations(|| {
        let mut trace = Vec::new();
        for &point in &points {
            trace.push(std::hint::black_box(point));
        }
        trace
    });
    assert_eq!(trace, points, "the calibration replays the trusted hook points");
    counted(allocations).expect("the trace calibration counts the effects buffer's allocations")
}

fn pin_generation(root: &Path, slot: CatalogSlotV1) -> GenerationLeaseV1 {
    DurableDirectoryV1::open(root)
        .expect("open the scaling directory for a reader pin")
        .generation_leases()
        .acquire(GenerationLeaseKeyV1::from_catalog(slot))
        .expect("pin the current generation")
}

fn reclaimed_files(slot: CatalogSlotV1) -> Vec<String> {
    let pack = slot.pack();
    let packs = usize::try_from(live_packs(slot)).expect("a live pack count fits usize");
    let mut names = Vec::with_capacity(packs + 2);
    names.extend(
        (pack.first_live_pack()..=pack.active_pack())
            .map(|id| pack_file_name(PackId::new(id).expect("a live pack id is nonzero"))),
    );
    names.push(index_file_name(
        IndexGenerationV1::new(pack.index_generation())
            .expect("a catalog names a nonzero index generation"),
    ));
    names.push(RECLAMATION_INTENT.to_owned());
    names.sort_unstable();
    names
}

fn live_packs(slot: CatalogSlotV1) -> u64 {
    let pack = slot.pack();
    pack.active_pack()
        .checked_sub(pack.first_live_pack())
        .and_then(|span| span.checked_add(1))
        .expect("the first live pack precedes the active pack")
}

fn checkpoint_core(
    mut lease: DurableWriterLeaseV1,
    core: Arc<Core>,
    limits: DurableLimits,
) -> (DurableWriterLeaseV1, Arc<Core>) {
    let source = acquire_cold_checked(
        &mut lease,
        &core,
        limits,
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("the cold-checked acquire before the eviction checkpoint");
    drop(core);
    let (lease, source) = SemanticallyAdmittedDurableWriterV1::from_parts(lease, source)
        .expect("an admitted source rebinds its own lease")
        .publish_generation_checkpoint_v1(
            limits,
            LogPackDecodeLimits::UNBOUNDED,
            DurableAdmissionLimitsV1::UNBOUNDED,
        )
        .expect("publish the eviction checkpoint")
        .into_parts();
    (lease, source.into_core())
}

fn checkpoint_chain<'fixture>(
    chain: ScalingChain<'fixture>,
    limits: DurableLimits,
) -> ScalingChain<'fixture> {
    let (lease, core) = checkpoint_core(chain.lease, chain.core, limits);
    assert_eq!(
        core.authority().slot().state(),
        chain.state,
        "the eviction checkpoint keeps the state"
    );
    ScalingChain {
        lease,
        core,
        ..chain
    }
}

fn decimal(values: &mut Vec<String>, numbers: impl IntoIterator<Item = u64>) {
    values.extend(numbers.into_iter().map(|number| number.to_string()));
}

fn emit(tag: &str, keys: &[&str], values: &[String]) {
    assert_eq!(keys.len(), values.len(), "a {tag} record carries one value per key");
    let length = keys
        .iter()
        .zip(values)
        .map(|(key, value)| key.len() + value.len() + 2)
        .sum::<usize>() +
        tag.len() +
        RECORD_VERSION.len() +
        2;
    let mut line = String::with_capacity(length);
    line.push_str(tag);
    line.push('\t');
    line.push_str(RECORD_VERSION);
    for (key, value) in keys.iter().zip(values) {
        assert!(!value.contains(['\t', '\n']), "a {tag} value holds a separator");
        line.push('\t');
        line.push_str(key);
        line.push('=');
        line.push_str(value);
    }
    line.push('\n');
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(line.as_bytes())
        .expect("write a scale record");
    stdout.flush().expect("flush a scale record");
}

fn emit_header(mode: Mode, cell_text: &str) {
    emit(HEADER, &HEADER_KEYS, &[mode.name().to_owned(), cell_text.to_owned()]);
}

fn verify_values(
    commit: usize,
    label: Label,
    record: &CommitRecord,
    costs: &[PhaseCost; 4],
    unlinked: &[String],
) -> Vec<String> {
    let mut values = Vec::with_capacity(VERIFY_KEYS.len());
    values.push(commit.to_string());
    values.push(label.name().to_owned());
    values.push(record.target.digest().to_string());
    decimal(
        &mut values,
        record
            .work
            .iter()
            .flat_map(|work| [work.references, work.expansions, work.quoted_bytes]),
    );
    decimal(&mut values, [
        record.logical_writes,
        record.metadata_bytes,
        record.fresh_objects,
        record.fresh_bytes,
        record.pack_delta,
        record.index_delta,
        record.live_packs,
        record.old_path,
        record.new_path,
        record.deleted_objects,
        record.rewrite.loaded_nodes,
        record.rewrite.edited_nodes,
        record.rewrite.emitted_nodes,
        record.rewrite.exact_path_chunks,
    ]);
    for (phase, cost) in PHASES.into_iter().zip(costs) {
        let allocations = counted(cost.allocations).unwrap_or_else(|| {
            panic!("the {phase:?} phase of commit {commit} counted no allocation")
        });
        decimal(&mut values, [
            allocations.count,
            allocations.total_bytes,
            allocations.peak_live_bytes,
            allocations.max_single_bytes,
            cost.io.syscr,
            cost.io.syscw,
            cost.io.rchar,
            cost.io.wchar,
        ]);
    }
    values.push(match unlinked {
        [] => "-".to_owned(),
        names => names.join(";"),
    });
    values
}

fn build(cell: ScaleCell, cell_text: &str, fixture_directory: &Path) {
    classify_directory(fixture_directory);
    require_empty(fixture_directory);
    emit_header(Mode::Build, cell_text);
    let started = Instant::now();
    let fixture = ScalingFixture::build(cell.writer, cell.keys_total, cell.history);
    let build_ns = wall_ns(started);
    install_checkpoint_image(fixture_directory, &fixture.image);
    let mut values = Vec::with_capacity(FIXTURE_KEYS.len());
    decimal(&mut values, [
        wide(fixture.keys.len()),
        wide(fixture.closure.len()),
        fixture.current.canonical_bytes(),
        wide(fixture.historical.len()),
        wide(fixture.union.len()),
        wide(fixture.image.pack.len()),
        wide(fixture.image.canonical_index.len()),
        wide(fixture.image.records.len()),
        build_ns,
    ]);
    emit(FIXTURE, &FIXTURE_KEYS, &values);
}

fn verify(cell: ScaleCell, cell_text: &str, fixture_directory: &Path, scratch: &Path) {
    classify_directory(fixture_directory);
    let filesystem = classify_directory(scratch);
    require_empty(scratch);
    emit_header(Mode::Verify, cell_text);
    let fixture = ScalingFixture::build(cell.writer, cell.keys_total, cell.history);
    require_fixture_files(fixture_directory, &fixture);
    install_checkpoint_image(scratch, &fixture.image);
    let limits = scale_limits(
        &cell,
        wide(fixture.image.pack.len()),
        wide(fixture.image.canonical_index.len()),
    );
    let started = Instant::now();
    let (lease, core) = admit_scaling_directory(scratch, cell.writer, limits);
    let admission_ns = wall_ns(started);
    let mut chain = ScalingChain::new(&fixture, scratch.to_path_buf(), lease, core);
    let probe_io = probe_io_cost();
    let probe_trace = probe_trace_allocations();
    let schedule = Schedule::new(cell);
    let mut probe = MeasuredProbe::default();
    let mut window = None;
    let mut pin = None;
    let mut reclaimed = Vec::new();
    for commit in 0..cell.commits {
        let label = cell.label(commit);
        if commit == cell.warmup {
            if let Case::Evict = cell.case {
                let slot = chain.core.authority().slot();
                pin = Some(pin_generation(scratch, slot));
                reclaimed = reclaimed_files(slot);
                chain = checkpoint_chain(chain, limits);
            }
            window = Some(Window::open());
        }
        if let Label::Reclaim = label {
            drop(
                pin.take()
                    .expect("the eviction pin is held until the reclaim commit"),
            );
        }
        let planned = chain.plan(&schedule.edits(commit));
        let before = directory_names(scratch);
        let record = chain
            .commit(planned, &mut probe)
            .unwrap_or_else(|failure| panic!("verified commit {commit} failed: {failure}"));
        let after = directory_names(scratch);
        let unlinked = before.difference(&after).cloned().collect::<Vec<_>>();
        match label {
            Label::Reclaim => assert_eq!(
                unlinked, reclaimed,
                "the reclaim commit did not unlink exactly the superseded generation"
            ),
            Label::Warmup | Label::Deferred | Label::Timed => {
                assert!(unlinked.is_empty(), "commit {commit} unlinked {unlinked:?}")
            }
        }
        emit(
            VERIFY,
            &VERIFY_KEYS,
            &verify_values(commit, label, &record, &probe.take(), &unlinked),
        );
    }
    let window = window
        .expect("the window opens before the first post-warm-up commit")
        .close();
    ProcessRecord {
        mode: Mode::Verify,
        commits: cell.commits,
        admission_ns,
        window,
        live_packs: live_packs(chain.core.authority().slot()),
        filesystem,
        probes: [
            probe_io.syscr,
            probe_io.syscw,
            probe_io.rchar,
            probe_io.wchar,
            probe_trace.count,
            probe_trace.total_bytes,
        ],
    }
    .emit();
}

fn run_timed(
    cell: ScaleCell,
    root: &Path,
    limits: DurableLimits,
    (mut lease, mut core): (DurableWriterLeaseV1, Arc<Core>),
    admission_ns: u64,
) -> TimedRun {
    let schedule = Schedule::new(cell);
    let epoch = Instant::now();
    let mut samples = Vec::with_capacity(cell.commits);
    let mut window = None;
    let mut pin = None;
    for commit in 0..cell.commits {
        let label = cell.label(commit);
        if commit == cell.warmup {
            if let Case::Evict = cell.case {
                pin = Some(pin_generation(root, core.authority().slot()));
                (lease, core) = checkpoint_core(lease, core, limits);
            }
            window = Some(Window::open());
        }
        if let Label::Reclaim = label {
            drop(
                pin.take()
                    .expect("the eviction pin is held until the reclaim commit"),
            );
        }
        let edits = schedule.edits(commit);
        let keys = edits
            .iter()
            .map(|&(index, _)| scaling_key(index))
            .collect::<Vec<_>>();
        let mut transitions = keys
            .iter()
            .zip(&edits)
            .map(|(key, &(_, after))| (key.as_slice(), 1, after))
            .collect::<Vec<_>>();
        let before = core.authority().slot();
        let batch = mutation_batch(before.state(), &mut transitions);
        let sequence = before
            .commit_sequence()
            .successor()
            .expect("the published commit sequence has a successor");
        let sealing = Reading::now(epoch);
        let sealed = OpenDagTransaction::new(
            core.verified_state(),
            MutationBatchViewV1::parse(&batch).expect("parse the timed mutation"),
            DagTransactionLimits::UNBOUNDED,
        )
        .and_then(OpenDagTransaction::seal)
        .expect("seal the timed mutation on the trusted core");
        let binding = Reading::now(epoch);
        let bound = core
            .bind_trusted_transaction(&lease, sealed, TrustedSuccessorPolicyV1::UNBOUNDED)
            .expect("bind the timed commit under the unbounded policy");
        let preparing = Reading::now(epoch);
        let prepared = core
            .prepare_transaction(&lease, &bound, sequence, LogPackDecodeLimits::UNBOUNDED)
            .expect("prepare the timed commit");
        let publishing = Reading::now(epoch);
        let published = Arc::new(
            publish_trusted_incremental_v1(
                &mut lease,
                &core,
                &prepared,
                TrustedSuccessorPolicyV1::UNBOUNDED,
                |_| {},
            )
            .expect("publish the timed commit"),
        );
        let releasing = Reading::now(epoch);
        drop(prepared);
        drop(bound);
        drop(std::mem::replace(&mut core, published));
        let released = Reading::now(epoch);
        samples.push(Sample {
            label,
            target: core.authority().slot().state().manifest().digest(),
            readings: [sealing, binding, preparing, publishing, releasing, released],
            rss_after: read_memory().rss,
        });
    }
    let window = window
        .expect("the window opens before the first post-warm-up commit")
        .close();
    TimedRun {
        admission_ns,
        samples,
        window,
        live_packs: live_packs(core.authority().slot()),
    }
}

fn time(cell: ScaleCell, cell_text: &str, fixture_directory: &Path, scratch: &Path) {
    classify_directory(fixture_directory);
    let filesystem = classify_directory(scratch);
    require_empty(scratch);
    emit_header(Mode::Time, cell_text);
    let (pack_bytes, index_bytes) = copy_fixture(fixture_directory, scratch);
    let limits = scale_limits(&cell, pack_bytes, index_bytes);
    let (ready_sender, ready_receiver) = mpsc::sync_channel::<()>(0);
    let (start_sender, start_receiver) = mpsc::sync_channel::<()>(0);
    let root = scratch.to_path_buf();
    let worker = std::thread::Builder::new()
        .name("trusted-scaling-time".to_owned())
        .stack_size(WORKER_STACK_BYTES)
        .spawn(move || {
            let started = Instant::now();
            let admitted = admit_scaling_directory(&root, cell.writer, limits);
            let admission_ns = wall_ns(started);
            ready_sender
                .send(())
                .expect("the timed worker signals readiness");
            start_receiver
                .recv()
                .expect("the timed worker receives the start signal");
            run_timed(cell, &root, limits, admitted, admission_ns)
        })
        .expect("spawn the timed worker");
    ready_receiver
        .recv()
        .expect("the timed worker becomes ready");
    start_sender
        .send(())
        .expect("the timed worker accepts the start signal");
    let run = worker.join().expect("the timed worker completes");
    let durations = run
        .samples
        .iter()
        .enumerate()
        .map(|(commit, sample)| {
            phase_durations(&sample.readings).unwrap_or_else(|| {
                panic!("commit {commit}: a clock went backwards or the wall clock did not advance")
            })
        })
        .collect::<Vec<_>>();
    for (commit, (sample, durations)) in run.samples.iter().zip(&durations).enumerate() {
        let mut values = Vec::with_capacity(SAMPLE_KEYS.len());
        values.push(commit.to_string());
        values.push(sample.label.name().to_owned());
        values.push(sample.target.to_string());
        decimal(&mut values, durations.iter().flatten().copied());
        decimal(&mut values, [sample.rss_after]);
        emit(SAMPLE, &SAMPLE_KEYS, &values);
    }
    ProcessRecord {
        mode: Mode::Time,
        commits: cell.commits,
        admission_ns: run.admission_ns,
        window: run.window,
        live_packs: run.live_packs,
        filesystem,
        probes: [0; 6],
    }
    .emit();
}

fn record_keys(tag: &str) -> Option<&'static [&'static str]> {
    match tag {
        HEADER => Some(&HEADER_KEYS),
        FIXTURE => Some(&FIXTURE_KEYS),
        VERIFY => Some(&VERIFY_KEYS),
        SAMPLE => Some(&SAMPLE_KEYS),
        PROCESS => Some(&PROCESS_KEYS),
        _ => None,
    }
}

fn parse_records(stdout: &str) -> Vec<Record> {
    stdout
        .lines()
        .filter(|line| line.starts_with(RECORD_PREFIX))
        .map(|line| {
            let mut fields = line.split('\t');
            let tag = fields.next().expect("a record line has a tag");
            let keys = record_keys(tag).unwrap_or_else(|| panic!("unknown record in {line}"));
            assert_eq!(fields.next(), Some(RECORD_VERSION), "record version in {line}");
            let values = keys
                .iter()
                .map(|key| {
                    let (name, value) = fields
                        .next()
                        .and_then(|field| field.split_once('='))
                        .unwrap_or_else(|| panic!("record {line} lacks {key}"));
                    assert_eq!(name, *key, "record {line} reorders its keys");
                    value.to_owned()
                })
                .collect::<Vec<_>>();
            assert!(fields.next().is_none(), "record {line} carries an extra field");
            Record {
                tag: tag.to_owned(),
                keys,
                values,
            }
        })
        .collect()
}

fn fresh_directory(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .expect("create a harness directory");
    path
}

fn run_child(cell_text: &str, mode: Mode, fixture: &Path, scratch: Option<&Path>) -> Vec<Record> {
    let mut command =
        Command::new(std::env::current_exe().expect("the current test executable resolves"));
    command
        .args(CHILD_ARGUMENTS)
        .env(CELL_ENV, cell_text)
        .env(MODE_ENV, mode.name())
        .env(FIXTURE_ENV, fixture);
    match scratch {
        Some(scratch) => command.env(SCRATCH_ENV, scratch),
        None => command.env_remove(SCRATCH_ENV),
    };
    let output = command.output().expect("the scale child starts");
    let stdout = String::from_utf8(output.stdout).expect("the scale child prints UTF-8");
    assert!(
        output.status.success(),
        "the {} child for {cell_text} failed:\nstdout:\n{stdout}\nstderr:\n{}",
        mode.name(),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_records(&stdout)
}

fn check_header(header: &Record, mode: Mode, cell_text: &str) {
    assert_eq!(header.tag, HEADER, "a child's first record is its header");
    assert_eq!(header.get("mode"), mode.name());
    assert_eq!(header.get("cell"), cell_text);
}

fn check_target(record: &Record) {
    let target = record.get("target");
    assert!(
        target.len() == 64 &&
            target
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')),
        "{target} is not a lowercase hex digest"
    );
}

fn check_process(process: &Record, mode: Mode, cell: &ScaleCell) {
    assert_eq!(process.tag, PROCESS, "a child's last record is its process record");
    assert_eq!(process.get("mode"), mode.name());
    assert_eq!(process.integer("commits"), wide(cell.commits));
    for key in PROCESS_KEYS
        .into_iter()
        .filter(|key| !matches!(*key, "mode" | "filesystem"))
    {
        process.integer(key);
    }
    assert!(process.integer("admission_ns") > 0);
    assert!(process.integer("hwm_reset") <= 1);
    assert!(process.integer("live_packs") >= 1);
    assert!(matches!(process.get("filesystem"), "ext4" | "xfs" | "btrfs" | "other"));
}

fn check_build(records: &[Record], cell: &ScaleCell, cell_text: &str) {
    let [header, fixture] = records else {
        panic!("a build child prints a header and a fixture record, not {} records", records.len());
    };
    check_header(header, Mode::Build, cell_text);
    assert_eq!(fixture.tag, FIXTURE);
    for key in FIXTURE_KEYS {
        assert!(fixture.integer(key) > 0, "the fixture record's {key} is zero");
    }
    assert_eq!(fixture.integer("keys_total"), wide(cell.keys_total));
    assert_eq!(fixture.integer("history"), wide(cell.history));
}

fn check_verify(records: &[Record], cell: &ScaleCell, cell_text: &str) -> Vec<String> {
    let (header, rest) = records
        .split_first()
        .expect("a verify child prints records");
    let (process, commits) = rest
        .split_last()
        .expect("a verify child prints a process record");
    check_header(header, Mode::Verify, cell_text);
    assert_eq!(commits.len(), cell.commits, "a verify child prints one record per commit");
    for (commit, record) in commits.iter().enumerate() {
        assert_eq!(record.tag, VERIFY);
        assert_eq!(record.integer("commit"), wide(commit));
        assert_eq!(record.get("label"), cell.label(commit).name());
        check_target(record);
        for key in VERIFY_KEYS
            .into_iter()
            .filter(|key| !matches!(*key, "label" | "target" | "unlinked"))
        {
            record.integer(key);
        }
        assert_eq!(
            record.integer("cur_exp"),
            record.integer("old_path") +
                record.integer("new_path") +
                record.integer("deleted_objects")
        );
        match cell.deletion {
            0 => assert_eq!(record.integer("deleted_objects"), 0),
            _ => assert!(record.integer("deleted_objects") > 0),
        }
        for prefix in PROBED_PHASE_PREFIXES {
            assert!(record.integer(&format!("{prefix}alloc_count")) > 0);
        }
        match (cell.label(commit), record.get("unlinked")) {
            (Label::Reclaim, unlinked) => assert!(
                unlinked.split(';').any(|name| name == RECLAMATION_INTENT),
                "the reclaim commit unlinked {unlinked}"
            ),
            (_, unlinked) => assert_eq!(unlinked, "-", "commit {commit} unlinked files"),
        }
    }
    check_process(process, Mode::Verify, cell);
    assert!(process.integer("probe_syscr") > 0);
    assert!(process.integer("probe_alloc_count") > 0);
    commits
        .iter()
        .map(|record| record.get("target").to_owned())
        .collect()
}

fn check_time(records: &[Record], cell: &ScaleCell, cell_text: &str) -> Vec<String> {
    let (header, rest) = records.split_first().expect("a time child prints records");
    let (process, samples) = rest
        .split_last()
        .expect("a time child prints a process record");
    check_header(header, Mode::Time, cell_text);
    assert_eq!(samples.len(), cell.commits, "a time child prints one sample per commit");
    for (commit, record) in samples.iter().enumerate() {
        assert_eq!(record.tag, SAMPLE);
        assert_eq!(record.integer("commit"), wide(commit));
        assert_eq!(record.get("label"), cell.label(commit).name());
        check_target(record);
        for key in SAMPLE_KEYS.into_iter().filter(|key| key.ends_with("_ns")) {
            let duration = record.integer(key);
            if key.ends_with("_wall_ns") {
                assert!(duration > 0, "commit {commit} has a zero {key}");
            }
        }
        assert!(record.integer("rss_after") > 0);
    }
    check_process(process, Mode::Time, cell);
    for key in PROBE_KEYS {
        assert_eq!(process.integer(key), 0, "a time child reports a nonzero {key}");
    }
    samples
        .iter()
        .map(|record| record.get("target").to_owned())
        .collect()
}

fn same_except_probes(first: &[Record], second: &[Record]) {
    assert_eq!(first.len(), second.len(), "the two verify runs print different record counts");
    for (left, right) in first
        .iter()
        .zip(second)
        .filter(|(left, _)| left.tag != PROCESS)
    {
        assert_eq!(left.tag, right.tag);
        for (key, (left_value, right_value)) in
            left.keys.iter().zip(left.values.iter().zip(&right.values))
        {
            if PROBED_PHASE_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
            {
                continue;
            }
            assert_eq!(left_value, right_value, "the two verify runs differ at {key}");
        }
    }
}

fn without_line(text: &str, field: &str) -> String {
    text.lines().filter(|line| !line.starts_with(field)).fold(
        String::with_capacity(text.len()),
        |mut kept, line| {
            kept.push_str(line);
            kept.push('\n');
            kept
        },
    )
}

#[test]
#[ignore = "driven by scripts/bench/rspace-fixed-delta-scaling.py"]
fn scale_child() {
    let Some(cell_text) = std::env::var_os(CELL_ENV) else {
        return;
    };
    let cell_text = cell_text.into_string().expect("the scale cell is UTF-8");
    let cell = parse_cell(&cell_text).unwrap_or_else(|| panic!("malformed scale cell {cell_text}"));
    let fixture =
        PathBuf::from(std::env::var_os(FIXTURE_ENV).expect("the scale fixture directory is set"));
    let scratch = std::env::var_os(SCRATCH_ENV).map(PathBuf::from);
    match (
        std::env::var(MODE_ENV)
            .expect("the scale mode is set")
            .as_str(),
        scratch,
    ) {
        ("build", None) => build(cell, &cell_text, &fixture),
        ("verify", Some(scratch)) => verify(cell, &cell_text, &fixture, &scratch),
        ("time", Some(scratch)) => time(cell, &cell_text, &fixture, &scratch),
        (mode, scratch) => panic!("scale mode {mode} with scratch {scratch:?} is no child"),
    }
}

#[test]
fn scaling_harness_is_deterministic_and_fails_closed() {
    let mut io_buffer = [0_u8; IO_BUFFER_BYTES];
    assert!(
        read_proc("/proc/self/io", &mut io_buffer)
            .and_then(LinuxIo::parse)
            .is_some(),
        "the live /proc/self/io parses"
    );
    let mut status_buffer = [0_u8; STATUS_BUFFER_BYTES];
    assert!(
        read_proc("/proc/self/status", &mut status_buffer)
            .and_then(parse_memory)
            .is_some(),
        "the live /proc/self/status parses"
    );
    let mut short_buffer = [0_u8; 64];
    assert!(
        read_proc("/proc/self/status", &mut short_buffer).is_none(),
        "a /proc read that fills its buffer is refused"
    );
    assert!(LinuxIo::parse(IO_SAMPLE).is_some());
    for field in IO_FIELDS {
        assert!(
            LinuxIo::parse(&without_line(IO_SAMPLE, field)).is_none(),
            "a /proc/self/io copy without {field} parses"
        );
    }
    let last_value = IO_SAMPLE
        .find("\nwrite_bytes:")
        .expect("the sample carries write_bytes") +
        "\nwrite_bytes: ".len();
    for length in 0..=last_value {
        assert!(
            LinuxIo::parse(&IO_SAMPLE[..length]).is_none(),
            "a /proc/self/io copy truncated to {length} bytes parses"
        );
    }
    assert!(parse_memory(STATUS_SAMPLE).is_some());
    for field in STATUS_FIELDS {
        assert!(
            parse_memory(&without_line(STATUS_SAMPLE, field)).is_none(),
            "a /proc/self/status copy without {field} parses"
        );
    }
    assert_eq!(positive(7, 7), None);
    assert_eq!(positive(8, 7), None);
    assert_eq!(positive(7, 8), Some(1));
    assert_eq!(non_negative(7, 7), Some(0));
    assert_eq!(non_negative(8, 7), None);
    assert_eq!(non_negative(7, 8), Some(1));
    assert!(counted(measure_allocations(|| ()).1).is_none());
    assert!(counted(measure_allocations(|| Vec::<u8>::with_capacity(1)).1).is_some());
    assert_eq!(classify_filesystem(TMPFS_MAGIC), None);
    assert_eq!(classify_filesystem(RAMFS_MAGIC), None);
    assert_eq!(classify_filesystem(EXT4_MAGIC), Some("ext4"));
    assert_eq!(classify_filesystem(XFS_MAGIC), Some("xfs"));
    assert_eq!(classify_filesystem(BTRFS_MAGIC), Some("btrfs"));
    assert_eq!(classify_filesystem(0x1234), Some("other"));
    for cell in MALFORMED_CELLS {
        assert_eq!(parse_cell(cell), None, "the malformed cell {cell:?} parses");
    }
    for length in 1..=64 {
        for seed in [0, 1, 7929, u64::MAX] {
            let permutation = Permutation::new(seed, length);
            let mut seen = vec![false; length];
            for index in 0..length {
                let position = permutation.position(index);
                assert!(
                    !std::mem::replace(&mut seen[position], true),
                    "seed {seed} repeats position {position} of {length}"
                );
            }
        }
    }

    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rspace++ has a workspace root")
        .join("target/verification/trusted-scaling");
    let scratch = test_scratch::acquire_in(&base, "rspace-trusted-scaling-");
    classify_directory(scratch.path());
    for (index, cell_text) in HARNESS_CELLS.into_iter().enumerate() {
        let cell = parse_cell(cell_text).expect("a harness cell parses");
        let fixture = fresh_directory(scratch.path(), &format!("{index}-fixture"));
        check_build(&run_child(cell_text, Mode::Build, &fixture, None), &cell, cell_text);
        let first = run_child(
            cell_text,
            Mode::Verify,
            &fixture,
            Some(&fresh_directory(scratch.path(), &format!("{index}-verify-1"))),
        );
        let second = run_child(
            cell_text,
            Mode::Verify,
            &fixture,
            Some(&fresh_directory(scratch.path(), &format!("{index}-verify-2"))),
        );
        let timed = run_child(
            cell_text,
            Mode::Time,
            &fixture,
            Some(&fresh_directory(scratch.path(), &format!("{index}-time"))),
        );
        let targets = check_verify(&first, &cell, cell_text);
        assert_eq!(check_verify(&second, &cell, cell_text), targets);
        same_except_probes(&first, &second);
        assert_eq!(
            check_time(&timed, &cell, cell_text),
            targets,
            "the timed target chain differs from the verified chain"
        );
    }
    fs::remove_dir_all(scratch.path()).expect("remove the scaling harness scratch directory");
}

#[test]
fn phase_durations_accept_zero_cpu_time_and_fail_closed() {
    const READINGS: [[u64; 3]; 6] = [
        [100, 1_000, 10_000],
        [101, 1_020, 10_300],
        [103, 1_050, 10_700],
        [106, 1_090, 11_200],
        [110, 1_140, 11_800],
        [115, 1_200, 12_500],
    ];
    const ADVANCING: [[u64; 3]; 5] =
        [[1, 20, 300], [2, 30, 400], [3, 40, 500], [4, 50, 600], [5, 60, 700]];
    let durations = |readings: [[u64; 3]; 6]| {
        phase_durations(&readings.map(|[thread, process, wall]| Reading {
            thread,
            process,
            wall,
        }))
    };
    assert_eq!(durations(READINGS), Some(ADVANCING));
    for (phase, [thread, process, wall]) in ADVANCING.into_iter().enumerate() {
        let (start, end) = (READINGS[phase], READINGS[phase + 1]);
        let cases = [
            ("the thread CPU clock stalls", [start[0], end[1], end[2]], Some([0, process, wall])),
            ("the process CPU clock stalls", [end[0], start[1], end[2]], Some([thread, 0, wall])),
            ("both CPU clocks stall", [start[0], start[1], end[2]], Some([0, 0, wall])),
            ("the wall clock stalls", [end[0], end[1], start[2]], None),
            ("the wall clock goes backwards", [end[0], end[1], start[2] - 1], None),
            ("the thread CPU clock goes backwards", [start[0] - 1, end[1], end[2]], None),
            ("the process CPU clock goes backwards", [end[0], start[1] - 1, end[2]], None),
        ];
        for (case, changed_end, expected) in cases {
            let mut readings = READINGS;
            readings[phase + 1] = changed_end;
            let measured = durations(readings).map(|durations| durations[phase]);
            assert_eq!(measured, expected, "the durations of phase {phase} when {case}");
        }
    }
}
