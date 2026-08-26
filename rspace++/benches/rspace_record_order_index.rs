use std::alloc::{GlobalAlloc, Layout, System};
use std::env;
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use rspace_plus_plus::rspace::order_index_experiment::{
    CollectionShape, OperationCell, OrderArm, OrderCase, OrderFixture, PrefixShape,
};

const SCHEMA: &str = "rspace-record-order-index-v2";
const WARMUPS: usize = 5;
const RETAINED_SETS: usize = 31;
const PROFILE_ITERATIONS: usize = 1_000;
const PROJECTION_REUSES: [usize; 4] = [1, 4, 16, 64];
const BALANCED_ARMS: [[OrderArm; 3]; 6] = [
    [OrderArm::SortedPrepared, OrderArm::SharedRecordOrderTrie, OrderArm::SplitPayloadOrderTrie],
    [OrderArm::SortedPrepared, OrderArm::SplitPayloadOrderTrie, OrderArm::SharedRecordOrderTrie],
    [OrderArm::SharedRecordOrderTrie, OrderArm::SortedPrepared, OrderArm::SplitPayloadOrderTrie],
    [OrderArm::SharedRecordOrderTrie, OrderArm::SplitPayloadOrderTrie, OrderArm::SortedPrepared],
    [OrderArm::SplitPayloadOrderTrie, OrderArm::SortedPrepared, OrderArm::SharedRecordOrderTrie],
    [OrderArm::SplitPayloadOrderTrie, OrderArm::SharedRecordOrderTrie, OrderArm::SortedPrepared],
];

const CASES: [OrderCase; 14] = [
    case(CollectionShape::Continuation, 1, 1, 64, 32, PrefixShape::Shared),
    case(CollectionShape::Continuation, 16, 1, 64, 32, PrefixShape::Shared),
    case(CollectionShape::Continuation, 256, 1, 64, 32, PrefixShape::Shared),
    case(CollectionShape::Continuation, 4_096, 1, 64, 32, PrefixShape::Shared),
    case(CollectionShape::Continuation, 256, 1, 16, 32, PrefixShape::Unshared),
    case(CollectionShape::Continuation, 256, 1, 4_096, 256, PrefixShape::Shared),
    case(CollectionShape::RestingJoin, 1, 1, 64, 32, PrefixShape::Shared),
    case(CollectionShape::RestingJoin, 16, 1, 64, 32, PrefixShape::Shared),
    case(CollectionShape::RestingJoin, 256, 1, 64, 32, PrefixShape::Shared),
    case(CollectionShape::RestingJoin, 4_096, 1, 64, 32, PrefixShape::Shared),
    case(CollectionShape::RestingJoin, 16, 16, 64, 32, PrefixShape::Shared),
    case(CollectionShape::RestingJoin, 1, 4_096, 64, 32, PrefixShape::Shared),
    case(CollectionShape::InstalledJoin, 16, 1, 64, 32, PrefixShape::Unshared),
    case(CollectionShape::InstalledJoin, 16, 16, 64, 256, PrefixShape::Shared),
];

const fn case(
    collection: CollectionShape,
    distinct: usize,
    multiplicity: usize,
    record_len: usize,
    prefix_len: usize,
    shape: PrefixShape,
) -> OrderCase {
    OrderCase {
        collection,
        distinct,
        multiplicity,
        record_len,
        prefix_len,
        shape,
    }
}

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static DEALLOC_CALLS: AtomicU64 = AtomicU64::new(0);
static DEALLOC_BYTES: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_alloc(layout.size());
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_alloc(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if !pointer.is_null() && COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
            DEALLOC_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(pointer, old, new_size) };
        if !next.is_null() {
            if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
                DEALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
                DEALLOC_BYTES.fetch_add(old.size() as u64, Ordering::Relaxed);
            }
            record_alloc(new_size);
        }
        next
    }
}

fn record_alloc(bytes: usize) {
    if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
        ALLOC_CALLS.fetch_add(1, Ordering::Relaxed);
        ALLOC_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    }
}

#[derive(Clone, Copy)]
struct TimeSample {
    set: usize,
    operation: OperationCell,
    arm: OrderArm,
    elapsed_ns: u128,
}

#[derive(Clone, Copy)]
struct AllocationSample {
    set: usize,
    operation: OperationCell,
    arm: OrderArm,
    allocation_bytes: u64,
    retained_bytes_floor: u64,
}

fn main() {
    if env::var_os("RSPACE_RECORD_ORDER_PROFILE_ARM").is_some() {
        profile();
    } else {
        matrix();
    }
}

fn matrix() {
    println!(
        "SCHEMA\t{SCHEMA}\tCHECK\tcase\tarm\tlogical\tprimary\tidentity\torder\\
         tretained_bytes_floor"
    );
    println!(
        "SCHEMA\t{SCHEMA}\tTIME\tcase\toperation\tset\tarm\telapsed_ns\tprimary\tidentity\torder\\
         tretained_bytes_floor"
    );
    println!(
        "SCHEMA\t{SCHEMA}\tALLOC\tcase\toperation\tset\tarm\talloc_calls\talloc_bytes\\
         tdealloc_calls\tdealloc_bytes"
    );
    println!("SCHEMA\t{SCHEMA}\tPROJECTION_TIME\tcase\treuse\tset\tarm\telapsed_ns");
    println!(
        "SCHEMA\t{SCHEMA}\tPROJECTION_ALLOC\tcase\treuse\tset\tarm\talloc_calls\talloc_bytes\\
         tdealloc_calls\tdealloc_bytes"
    );
    println!(
        "SCHEMA\t{SCHEMA}\tDECISION_CELL\tcase\toperation\tcandidate\tcontrol_median_ns\\
         tcandidate_median_ns\ttime_ratio_ppm\tpaired_p99_time_ratio_ppm\\
         tcontrol_median_alloc_bytes\tcandidate_median_alloc_bytes\tallocation_ratio_ppm\\
         tpaired_p99_allocation_ratio_ppm\tcontrol_retained_bytes_floor\\
         tcandidate_retained_bytes_floor\tretained_ratio_ppm\tstatus"
    );
    println!(
        "SCHEMA\t{SCHEMA}\tPROJECTION_DECISION\tcase\treuse\tcandidate\tcontrol_median_ns\\
         tcandidate_median_ns\ttime_ratio_ppm\tpaired_p99_time_ratio_ppm\tallocation_ratio_ppm\\
         tstatus"
    );
    for case in CASES {
        let fixture = OrderFixture::new(case);
        for arm in OrderArm::ALL {
            let diagnostics = fixture.diagnostics(arm);
            println!(
                "CHECK\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                case.label(),
                arm.label(),
                diagnostics.logical_records,
                diagnostics.canonical_endpoints,
                diagnostics.identity_endpoints,
                diagnostics.order_endpoints,
                diagnostics.retained.owned_payload_bytes_floor,
            );
        }
        projection_reuse_matrix(case, &fixture);
        for operation in OperationCell::ALL {
            for warmup in 0..WARMUPS {
                let arm = OrderArm::ALL[warmup % OrderArm::ALL.len()];
                let mut prepared = fixture.prepare(arm, operation);
                black_box(prepared.execute());
                prepared.verify();
            }

            let mut times = Vec::with_capacity(RETAINED_SETS * OrderArm::ALL.len());
            for set in 0..RETAINED_SETS {
                for arm in BALANCED_ARMS[set % BALANCED_ARMS.len()] {
                    let mut prepared = fixture.prepare(arm, operation);
                    let started = Instant::now();
                    let result = prepared.execute();
                    let elapsed_ns = started.elapsed().as_nanos();
                    black_box(result);
                    prepared.verify();
                    let retained = prepared.retained();
                    println!(
                        "TIME\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        case.label(),
                        operation.label(),
                        set,
                        arm.label(),
                        elapsed_ns,
                        retained.primary_endpoints,
                        retained.identity_endpoints,
                        retained.order_endpoints,
                        retained.owned_payload_bytes_floor,
                    );
                    times.push(TimeSample {
                        set,
                        operation,
                        arm,
                        elapsed_ns,
                    });
                }
            }

            let mut allocations = Vec::with_capacity(RETAINED_SETS * OrderArm::ALL.len());
            for set in 0..RETAINED_SETS {
                for arm in BALANCED_ARMS[set % BALANCED_ARMS.len()] {
                    let mut prepared = fixture.prepare(arm, operation);
                    reset_allocations();
                    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
                    let result = prepared.execute();
                    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
                    black_box(result);
                    prepared.verify();
                    let allocation_bytes = ALLOC_BYTES.load(Ordering::Relaxed);
                    let retained_bytes_floor = prepared.retained().owned_payload_bytes_floor;
                    println!(
                        "ALLOC\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        case.label(),
                        operation.label(),
                        set,
                        arm.label(),
                        ALLOC_CALLS.load(Ordering::Relaxed),
                        allocation_bytes,
                        DEALLOC_CALLS.load(Ordering::Relaxed),
                        DEALLOC_BYTES.load(Ordering::Relaxed),
                    );
                    allocations.push(AllocationSample {
                        set,
                        operation,
                        arm,
                        allocation_bytes,
                        retained_bytes_floor,
                    });
                }
            }
            emit_decisions(case, operation, &times, &allocations);
        }
    }
}

fn projection_reuse_matrix(case: OrderCase, fixture: &OrderFixture) {
    for reuse in PROJECTION_REUSES {
        for warmup in 0..WARMUPS {
            let arm = OrderArm::ALL[warmup % OrderArm::ALL.len()];
            fixture.audit_outcome(&black_box(fixture.project(arm, reuse)));
        }
        let mut times = Vec::with_capacity(RETAINED_SETS * OrderArm::ALL.len());
        for set in 0..RETAINED_SETS {
            for arm in BALANCED_ARMS[set % BALANCED_ARMS.len()] {
                let started = Instant::now();
                let outcome = fixture.project(arm, reuse);
                let elapsed_ns = started.elapsed().as_nanos();
                fixture.audit_outcome(&outcome);
                black_box(outcome);
                println!(
                    "PROJECTION_TIME\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}",
                    case.label(),
                    reuse,
                    set,
                    arm.label(),
                    elapsed_ns,
                );
                times.push(TimeSample {
                    set,
                    operation: OperationCell::Project,
                    arm,
                    elapsed_ns,
                });
            }
        }
        let mut allocations = Vec::with_capacity(RETAINED_SETS * OrderArm::ALL.len());
        for set in 0..RETAINED_SETS {
            for arm in BALANCED_ARMS[set % BALANCED_ARMS.len()] {
                reset_allocations();
                COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
                let outcome = fixture.project(arm, reuse);
                COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
                fixture.audit_outcome(&outcome);
                black_box(outcome);
                let diagnostics = fixture.diagnostics(arm);
                let allocation_bytes = ALLOC_BYTES.load(Ordering::Relaxed);
                println!(
                    "PROJECTION_ALLOC\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    case.label(),
                    reuse,
                    set,
                    arm.label(),
                    ALLOC_CALLS.load(Ordering::Relaxed),
                    allocation_bytes,
                    DEALLOC_CALLS.load(Ordering::Relaxed),
                    DEALLOC_BYTES.load(Ordering::Relaxed),
                );
                allocations.push(AllocationSample {
                    set,
                    operation: OperationCell::Project,
                    arm,
                    allocation_bytes,
                    retained_bytes_floor: diagnostics.retained.owned_payload_bytes_floor,
                });
            }
        }
        emit_projection_decisions(case, reuse, &times, &allocations);
    }
}

fn emit_projection_decisions(
    case: OrderCase,
    reuse: usize,
    times: &[TimeSample],
    allocations: &[AllocationSample],
) {
    let operation = OperationCell::Project;
    let control_ns = median_time(times, operation, OrderArm::SortedPrepared);
    let control_alloc = median_allocation(allocations, operation, OrderArm::SortedPrepared);
    for candidate in [OrderArm::SharedRecordOrderTrie, OrderArm::SplitPayloadOrderTrie] {
        let candidate_ns = median_time(times, operation, candidate);
        let time_ratio_ppm = ratio(candidate_ns, control_ns);
        let p99_time_ratio_ppm = paired_p99_time_ratio(times, operation, candidate);
        let candidate_alloc = median_allocation(allocations, operation, candidate);
        let allocation_ratio_ppm = ratio(candidate_alloc as u128, control_alloc as u128);
        println!(
            "PROJECTION_DECISION\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            case.label(),
            reuse,
            candidate.label(),
            control_ns,
            candidate_ns,
            time_ratio_ppm,
            p99_time_ratio_ppm,
            allocation_ratio_ppm,
            if time_ratio_ppm <= 1_050_000 && p99_time_ratio_ppm <= 1_050_000 {
                "read_cell_candidate"
            } else {
                "read_cell_rejected"
            },
        );
    }
}

fn reset_allocations() {
    ALLOC_CALLS.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
    DEALLOC_CALLS.store(0, Ordering::Relaxed);
    DEALLOC_BYTES.store(0, Ordering::Relaxed);
}

fn emit_decisions(
    case: OrderCase,
    operation: OperationCell,
    times: &[TimeSample],
    allocations: &[AllocationSample],
) {
    let control_ns = median_time(times, operation, OrderArm::SortedPrepared);
    let control_alloc = median_allocation(allocations, operation, OrderArm::SortedPrepared);
    let control_retained = median_retained(allocations, operation, OrderArm::SortedPrepared);
    for candidate in [OrderArm::SharedRecordOrderTrie, OrderArm::SplitPayloadOrderTrie] {
        let candidate_ns = median_time(times, operation, candidate);
        let time_ratio_ppm = ratio(candidate_ns, control_ns);
        let p99_time_ratio_ppm = paired_p99_time_ratio(times, operation, candidate);
        let candidate_alloc = median_allocation(allocations, operation, candidate);
        let allocation_ratio_ppm = ratio(candidate_alloc as u128, control_alloc as u128);
        let p99_allocation_ratio_ppm =
            paired_p99_allocation_ratio(allocations, operation, candidate);
        let candidate_retained = median_retained(allocations, operation, candidate);
        let retained_ratio_ppm = ratio(candidate_retained as u128, control_retained as u128);
        let status = if time_ratio_ppm <= 1_050_000 &&
            p99_time_ratio_ppm <= 1_050_000 &&
            allocation_ratio_ppm <= 1_050_000 &&
            p99_allocation_ratio_ppm <= 1_050_000 &&
            retained_ratio_ppm <= 1_050_000
        {
            "cell_pareto_candidate"
        } else {
            "requires_weighted_tradeoff"
        };
        println!(
            "DECISION_CELL\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            case.label(),
            operation.label(),
            candidate.label(),
            control_ns,
            candidate_ns,
            time_ratio_ppm,
            p99_time_ratio_ppm,
            control_alloc,
            candidate_alloc,
            allocation_ratio_ppm,
            p99_allocation_ratio_ppm,
            control_retained,
            candidate_retained,
            retained_ratio_ppm,
            status,
        );
    }
}

fn median_time(samples: &[TimeSample], operation: OperationCell, arm: OrderArm) -> u128 {
    let mut values = samples
        .iter()
        .filter(|sample| sample.operation == operation && sample.arm == arm)
        .map(|sample| sample.elapsed_ns)
        .collect::<Vec<_>>();
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_allocation(samples: &[AllocationSample], operation: OperationCell, arm: OrderArm) -> u64 {
    let mut values = samples
        .iter()
        .filter(|sample| sample.operation == operation && sample.arm == arm)
        .map(|sample| sample.allocation_bytes)
        .collect::<Vec<_>>();
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_retained(samples: &[AllocationSample], operation: OperationCell, arm: OrderArm) -> u64 {
    let mut values = samples
        .iter()
        .filter(|sample| sample.operation == operation && sample.arm == arm)
        .map(|sample| sample.retained_bytes_floor)
        .collect::<Vec<_>>();
    values.sort_unstable();
    values[values.len() / 2]
}

fn paired_p99_time_ratio(
    samples: &[TimeSample],
    operation: OperationCell,
    candidate: OrderArm,
) -> u128 {
    let mut ratios = (0..RETAINED_SETS)
        .map(|set| {
            let control = samples
                .iter()
                .find(|sample| {
                    sample.operation == operation &&
                        sample.arm == OrderArm::SortedPrepared &&
                        sample.set == set
                })
                .expect("balanced control observation")
                .elapsed_ns;
            let candidate_ns = samples
                .iter()
                .find(|sample| {
                    sample.operation == operation && sample.arm == candidate && sample.set == set
                })
                .expect("balanced candidate observation")
                .elapsed_ns;
            ratio(candidate_ns, control)
        })
        .collect::<Vec<_>>();
    ratios.sort_unstable();
    ratios[(ratios.len() * 99).div_ceil(100).saturating_sub(1)]
}

fn paired_p99_allocation_ratio(
    samples: &[AllocationSample],
    operation: OperationCell,
    candidate: OrderArm,
) -> u128 {
    let mut ratios = (0..RETAINED_SETS)
        .map(|set| {
            let control = samples
                .iter()
                .find(|sample| {
                    sample.operation == operation &&
                        sample.arm == OrderArm::SortedPrepared &&
                        sample.set == set
                })
                .expect("balanced control allocation observation")
                .allocation_bytes;
            let candidate_bytes = samples
                .iter()
                .find(|sample| {
                    sample.operation == operation && sample.arm == candidate && sample.set == set
                })
                .expect("balanced candidate allocation observation")
                .allocation_bytes;
            ratio(candidate_bytes as u128, control as u128)
        })
        .collect::<Vec<_>>();
    ratios.sort_unstable();
    ratios[(ratios.len() * 99).div_ceil(100).saturating_sub(1)]
}

fn ratio(candidate: u128, control: u128) -> u128 {
    match (candidate, control) {
        (0, 0) => 1_000_000,
        (_, 0) => u128::MAX,
        _ => candidate.saturating_mul(1_000_000) / control,
    }
}

fn profile() {
    let arm = parse_arm(&env::var("RSPACE_RECORD_ORDER_PROFILE_ARM").expect("profile arm"));
    let operation = env::var("RSPACE_RECORD_ORDER_PROFILE_OPERATION")
        .map_or(OperationCell::Project, |value| parse_operation(&value));
    let case_index = env::var("RSPACE_RECORD_ORDER_PROFILE_CASE")
        .map_or(0, |value| value.parse().expect("valid case index"));
    let iterations = env::var("RSPACE_RECORD_ORDER_PROFILE_ITERATIONS")
        .map_or(PROFILE_ITERATIONS, |value| value.parse().expect("valid iteration count"));
    let fixture = OrderFixture::new(CASES[case_index]);
    for _ in 0..iterations {
        let mut prepared = fixture.prepare(arm, operation);
        black_box(prepared.execute());
        prepared.verify();
    }
}

fn parse_arm(value: &str) -> OrderArm {
    match value {
        "sorted_prepared" => OrderArm::SortedPrepared,
        "shared_record_order_trie" => OrderArm::SharedRecordOrderTrie,
        "split_payload_order_trie" => OrderArm::SplitPayloadOrderTrie,
        other => panic!("unknown profile arm {other}"),
    }
}

fn parse_operation(value: &str) -> OperationCell {
    OperationCell::ALL
        .into_iter()
        .find(|operation| operation.label() == value)
        .unwrap_or_else(|| panic!("unknown profile operation {value}"))
}
