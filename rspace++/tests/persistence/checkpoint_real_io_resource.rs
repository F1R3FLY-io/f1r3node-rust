use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::Instant;

use shared::rust::test_scratch;

use super::checkpoint_wal::fixture_with_retained_union_cardinality;
use super::durable_publication::install_checkpoint_image;
use super::durable_recovery::{checkpoint_image_from_source, limits};
use super::kind1_recovery_protocol_properties::measure_allocations;
use super::support::TestSemanticProfile;
use crate::rspace::persistence::LogPackDecodeLimits;
use crate::rspace::persistence::durable::{DurableAdmissionLimitsV1, DurableDirectoryV1};

const CHILD_PHASE_ENV: &str = "RSPACE_CHECKPOINT_REAL_IO_CHILD_PHASE";
const CHILD_ROOT_ENV: &str = "RSPACE_CHECKPOINT_REAL_IO_CHILD_ROOT";
const CHILD_HISTORY_ENV: &str = "RSPACE_CHECKPOINT_REAL_IO_CHILD_HISTORY";
const CHILD_STACK_ENV: &str = "RSPACE_CHECKPOINT_REAL_IO_CHILD_STACK";
const CHILD_BUFFER_REFERENCE_ENV: &str = "RSPACE_CHECKPOINT_REAL_IO_CHILD_BUFFER_REFERENCE";
const FULL_MATRIX_ENV: &str = "RSPACE_CHECKPOINT_REAL_IO_FULL_MATRIX";
const SCRATCH_BASE_ENV: &str = "RSPACE_CHECKPOINT_REAL_IO_SCRATCH_BASE";
const HEAPTRACK_SPEC_ENV: &str = "RSPACE_CHECKPOINT_REAL_IO_HEAPTRACK_SPEC";
const HEAPTRACK_OUTPUT_ENV: &str = "RSPACE_CHECKPOINT_REAL_IO_HEAPTRACK_OUTPUT";
const TEST_NAME: &str =
    "rspace::persistence::tests::checkpoint_real_io_resource::checkpoint_real_io_cardinality_resource_gate";

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LinuxIo {
    rchar: u64,
    wchar: u64,
    syscr: u64,
    syscw: u64,
    read_bytes: u64,
    write_bytes: u64,
}

impl LinuxIo {
    pub(super) fn read() -> Option<Self> {
        let contents = std::fs::read_to_string("/proc/self/io").ok()?;
        let field = |name: &str| -> Option<u64> {
            contents
                .lines()
                .find_map(|line| line.strip_prefix(name))?
                .trim()
                .parse()
                .ok()
        };
        Some(Self {
            rchar: field("rchar:")?,
            wchar: field("wchar:")?,
            syscr: field("syscr:")?,
            syscw: field("syscw:")?,
            read_bytes: field("read_bytes:")?,
            write_bytes: field("write_bytes:")?,
        })
    }

    pub(super) fn difference(self, before: Self) -> Option<Self> {
        Some(Self {
            rchar: self.rchar.checked_sub(before.rchar)?,
            wchar: self.wchar.checked_sub(before.wchar)?,
            syscr: self.syscr.checked_sub(before.syscr)?,
            syscw: self.syscw.checked_sub(before.syscw)?,
            read_bytes: self.read_bytes.checked_sub(before.read_bytes)?,
            write_bytes: self.write_bytes.checked_sub(before.write_bytes)?,
        })
    }
}

pub(super) fn linux_status_bytes(field: &str) -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix(field))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
}

fn linux_open_descriptor_count() -> Option<usize> {
    Some(
        std::fs::read_dir("/proc/self/fd")
            .ok()?
            .filter_map(Result::ok)
            .count(),
    )
}

fn linux_resident_stack_span_bytes() -> Option<usize> {
    let mut attributes = MaybeUninit::<libc::pthread_attr_t>::uninit();
    if unsafe { libc::pthread_getattr_np(libc::pthread_self(), attributes.as_mut_ptr()) } != 0 {
        return None;
    }
    let mut attributes = unsafe { attributes.assume_init() };
    let mut stack_base = std::ptr::null_mut();
    let mut stack_size = 0;
    let status =
        unsafe { libc::pthread_attr_getstack(&attributes, &mut stack_base, &mut stack_size) };
    unsafe { libc::pthread_attr_destroy(&mut attributes) };
    if status != 0 {
        return None;
    }
    let page_size = usize::try_from(unsafe { libc::sysconf(libc::_SC_PAGESIZE) }).ok()?;
    if page_size == 0 || (stack_base as usize) % page_size != 0 || stack_size % page_size != 0 {
        return None;
    }
    let mut residency = vec![0u8; stack_size / page_size];
    if unsafe { libc::mincore(stack_base, stack_size, residency.as_mut_ptr()) } != 0 {
        return None;
    }
    let lowest_resident_page = residency.iter().position(|&page| page & 1 != 0)?;
    stack_size.checked_sub(lowest_resident_page * page_size)
}

pub(super) fn linux_filesystem_magic(path: &Path) -> u64 {
    let path = CString::new(path.as_os_str().as_bytes()).expect("scratch path has no NUL byte");
    let mut filesystem = MaybeUninit::<libc::statfs>::uninit();
    let result = unsafe { libc::statfs(path.as_ptr(), filesystem.as_mut_ptr()) };
    assert_eq!(result, 0, "statfs succeeds for the checkpoint scratch directory");
    let filesystem = unsafe { filesystem.assume_init() };
    (filesystem.f_type as u64) & 0xffff_ffff
}

fn scratch_base() -> PathBuf {
    std::env::var_os(SCRATCH_BASE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("rspace++ has a workspace root")
                .join("target/verification/checkpoint-real-io")
        })
}

fn run_child(
    phase: &str,
    root: &Path,
    historical_states: usize,
    stack_bytes: usize,
    buffer_reference_bytes: usize,
) {
    let retained_union_objects = historical_states
        .checked_add(2)
        .expect("historical state count fits retained-union cardinality");
    let retained_union_objects_u64 =
        u64::try_from(retained_union_objects).expect("retained-union cardinality fits u64");
    let object_records = retained_union_objects_u64
        .checked_add(1)
        .expect("checkpoint object count fits u64");
    let publish = match phase {
        "publication" => true,
        "recovery" => false,
        _ => panic!("unknown checkpoint real-I/O phase: {phase}"),
    };
    let (ready_sender, ready_receiver) = mpsc::sync_channel(0);
    let (start_sender, start_receiver) = mpsc::sync_channel(0);
    let root = root.to_path_buf();
    let worker = std::thread::Builder::new()
        .name(format!("checkpoint-real-io-{phase}-{historical_states}"))
        .stack_size(stack_bytes)
        .spawn(move || {
            if publish {
                let writer = DurableDirectoryV1::open(&root)
                    .expect("open checkpoint directory")
                    .recover_and_admit_v1::<TestSemanticProfile>(
                        limits(),
                        LogPackDecodeLimits::UNBOUNDED,
                        DurableAdmissionLimitsV1::UNBOUNDED,
                    )
                    .expect("admit initial checkpoint");
                assert_eq!(
                    writer.report().retained_union.distinct_objects,
                    retained_union_objects_u64,
                );
                ready_sender
                    .send(())
                    .expect("publication worker signals readiness");
                start_receiver
                    .recv()
                    .expect("publication worker receives start signal");
                let stack_span_before = linux_resident_stack_span_bytes();
                let (published, allocations) = measure_allocations(|| {
                    writer
                        .publish_generation_checkpoint_v1(
                            limits(),
                            LogPackDecodeLimits::UNBOUNDED,
                            DurableAdmissionLimitsV1::UNBOUNDED,
                        )
                        .expect("publish checkpoint to disk")
                });
                assert_eq!(published.authority().slot().pack().active_pack(), 2);
                assert_eq!(
                    published.report().retained_union.distinct_objects,
                    retained_union_objects_u64,
                );
                (allocations, stack_span_before, linux_resident_stack_span_bytes())
            } else {
                ready_sender
                    .send(())
                    .expect("recovery worker signals readiness");
                start_receiver
                    .recv()
                    .expect("recovery worker receives start signal");
                let stack_span_before = linux_resident_stack_span_bytes();
                let (recovered, allocations) = measure_allocations(|| {
                    DurableDirectoryV1::open(&root)
                        .expect("reopen checkpoint directory")
                        .recover_writer_v1(limits(), LogPackDecodeLimits::UNBOUNDED)
                        .expect("recover published checkpoint")
                });
                assert_eq!(recovered.recovered().checkpoints(), 1);
                assert_eq!(recovered.recovered().object_records(), object_records);
                assert_eq!(recovered.recovered().object_index().record_count(), object_records);
                (allocations, stack_span_before, linux_resident_stack_span_bytes())
            }
        })
        .expect("spawn bounded-stack checkpoint worker");
    ready_receiver
        .recv()
        .expect("checkpoint worker becomes ready");
    let rss_before = linux_status_bytes("VmRSS:");
    let peak_rss_before = linux_status_bytes("VmHWM:");
    let io_before = LinuxIo::read();
    let descriptors_before = linux_open_descriptor_count();
    let start = Instant::now();
    start_sender
        .send(())
        .expect("checkpoint worker receives start signal");
    let (allocations, stack_span_before, stack_span_after) = worker
        .join()
        .expect("bounded-stack checkpoint worker completes");
    let elapsed = start.elapsed();
    let io_after = LinuxIo::read();
    let descriptors_after = linux_open_descriptor_count();
    eprintln!(
        "checkpoint real-io resource: phase={phase} historical_states={historical_states} \
         retained_union_objects={retained_union_objects} object_records={object_records} \
         worker_stack_bytes={stack_bytes} elapsed_ns={} rss_before_bytes={rss_before:?} \
         rss_after_bytes={:?} peak_rss_before_bytes={peak_rss_before:?} peak_rss_after_bytes={:?} \
         descriptors_before={descriptors_before:?} descriptors_after={descriptors_after:?} \
         linux_io_delta={:?} allocation_calls={} allocation_bytes={} \
         allocation_max_single_bytes={} allocation_peak_live_bytes={} \
         allocation_live_at_end_bytes={} resident_stack_span_before_bytes={stack_span_before:?} \
         resident_stack_span_after_bytes={stack_span_after:?}",
        elapsed.as_nanos(),
        linux_status_bytes("VmRSS:"),
        linux_status_bytes("VmHWM:"),
        io_before
            .zip(io_after)
            .and_then(|(before, after)| after.difference(before)),
        allocations.count,
        allocations.total_bytes,
        allocations.max_single_bytes,
        allocations.peak_live_bytes,
        allocations.live_at_end,
    );
    if let (Some(before), Some(after)) = (descriptors_before, descriptors_after) {
        assert!(after <= before + 1, "checkpoint {phase} leaked descriptors: {before} -> {after}");
    }
    assert!(
        stack_span_before.is_some() && stack_span_after.is_some(),
        "checkpoint {phase} resident stack-page measurement failed",
    );
    if let Some(peak) = linux_status_bytes("VmHWM:") {
        assert!(peak <= 256 * 1024 * 1024, "checkpoint {phase} peak RSS {peak} exceeds 256 MiB");
    }
    // A full input image allocation must remain visible above the fixed 64 KiB
    // scratch allocation once the fixture is large enough to distinguish them.
    if buffer_reference_bytes > 256 * 1024 {
        assert!(
            allocations.max_single_bytes < buffer_reference_bytes as u64,
            "checkpoint {phase} requested {} bytes in one allocation for a {}-byte input pack",
            allocations.max_single_bytes,
            buffer_reference_bytes,
        );
    }
}

#[test]
fn checkpoint_real_io_cardinality_resource_gate() {
    if let Some(phase) = std::env::var_os(CHILD_PHASE_ENV) {
        let phase = phase.to_str().expect("child phase is UTF-8");
        let root = std::env::var_os(CHILD_ROOT_ENV).expect("child checkpoint directory is set");
        let historical_states = std::env::var(CHILD_HISTORY_ENV)
            .expect("child historical state count is set")
            .parse::<usize>()
            .expect("child historical state count is a usize");
        let stack_bytes = std::env::var(CHILD_STACK_ENV)
            .expect("child stack size is set")
            .parse::<usize>()
            .expect("child stack size is a usize");
        let buffer_reference_bytes = std::env::var(CHILD_BUFFER_REFERENCE_ENV)
            .expect("child buffer reference is set")
            .parse::<usize>()
            .expect("child buffer reference is a usize");
        run_child(phase, Path::new(&root), historical_states, stack_bytes, buffer_reference_bytes);
        return;
    }

    let cardinalities: &[usize] = if std::env::var(FULL_MATRIX_ENV).ok().as_deref() == Some("1") {
        &[1, 16, 256, 1_000, 4_096, 20_000]
    } else {
        &[1, 1_000]
    };
    let scratch_base = scratch_base();
    for &historical_states in cardinalities {
        for stack_bytes in [128 * 1024, 256 * 1024] {
            let scratch = test_scratch::acquire_in(&scratch_base, "rspace-checkpoint-real-io-");
            let filesystem_magic = linux_filesystem_magic(scratch.path());
            assert_ne!(filesystem_magic, 0x0102_1994, "checkpoint scratch is tmpfs");
            assert_ne!(filesystem_magic, 0x8584_58f6, "checkpoint scratch is ramfs");
            let filesystem = match filesystem_magic {
                0xef53 => "ext4",
                0x5846_5342 => "xfs",
                0x9123_683e => "btrfs",
                _ => "other",
            };
            eprintln!(
                "checkpoint real-io scratch: path={} filesystem={filesystem} \
                 filesystem_magic=0x{filesystem_magic:x}",
                scratch.path().display(),
            );
            let fixture_start = Instant::now();
            let retained_union_objects = historical_states + 2;
            let source = fixture_with_retained_union_cardinality(retained_union_objects);
            let image = checkpoint_image_from_source(source);
            let initial_pack_bytes = image.pack.len();
            let initial_index_bytes = image.canonical_index.len();
            install_checkpoint_image(scratch.path(), &image);
            drop(image);
            eprintln!(
                "checkpoint real-io fixture: historical_states={historical_states} \
                 retained_union_objects={retained_union_objects} worker_stack_bytes={stack_bytes} \
                 fixture_elapsed_ns={} initial_pack_bytes={initial_pack_bytes} \
                 initial_index_bytes={initial_index_bytes}",
                fixture_start.elapsed().as_nanos(),
            );

            for phase in ["publication", "recovery"] {
                let executable = std::env::current_exe().expect("current test executable");
                let selected = std::env::var(HEAPTRACK_SPEC_ENV).ok().is_some_and(|spec| {
                    spec == format!("{phase}:{historical_states}:{stack_bytes}")
                });
                let mut command = if selected {
                    let output = std::env::var_os(HEAPTRACK_OUTPUT_ENV).unwrap_or_else(|| {
                        scratch_base
                            .join("heaptrack-checkpoint-child-%p.gz")
                            .into_os_string()
                    });
                    let mut command = Command::new("heaptrack");
                    command
                        .arg("--record-only")
                        .arg("-o")
                        .arg(output)
                        .arg(executable);
                    command
                } else {
                    Command::new(executable)
                };
                let output = command
                    .arg("--exact")
                    .arg(TEST_NAME)
                    .arg("--nocapture")
                    .env(CHILD_PHASE_ENV, phase)
                    .env(CHILD_ROOT_ENV, scratch.path())
                    .env(CHILD_HISTORY_ENV, historical_states.to_string())
                    .env(CHILD_STACK_ENV, stack_bytes.to_string())
                    .env(CHILD_BUFFER_REFERENCE_ENV, initial_pack_bytes.to_string())
                    .output()
                    .expect("checkpoint real-I/O child starts");
                assert!(
                    output.status.success(),
                    "checkpoint real-I/O {phase}, historical states {historical_states}, stack \
                     {stack_bytes} failed:\nstdout:\n{}\nstderr:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                );
                eprint!("{}", String::from_utf8_lossy(&output.stderr));
            }
            std::fs::remove_dir_all(scratch.path())
                .expect("remove completed checkpoint real-I/O scratch directory");
        }
    }
}
