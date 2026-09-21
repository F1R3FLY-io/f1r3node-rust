use std::hint::black_box;
use std::io::Write;
use std::time::Instant;

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Set,
    Map,
    Not,
    Binder,
    Send,
    PathMap,
    SpatialList,
    SpatialBinder,
    SpatialPathMap,
}

const FAMILIES: [(&str, Family); 9] = [
    ("set", Family::Set),
    ("map", Family::Map),
    ("not", Family::Not),
    ("binder", Family::Binder),
    ("send", Family::Send),
    ("pathmap", Family::PathMap),
    ("spatial_list", Family::SpatialList),
    ("spatial_binder", Family::SpatialBinder),
    ("spatial_pathmap", Family::SpatialPathMap),
];

struct Fixture {
    target: Option<Par>,
    pattern: Option<Par>,
    env: Option<Env<Par>>,
}

enum Output {
    None,
    Par(Par),
    Sorted(ScoredTerm<Par>),
    Matched(SpatialMatcherContext),
}

impl Family {
    fn construct(self, depth: usize) -> Fixture {
        let target = match self {
            Self::Set => nested_sets(depth),
            Self::Map => nested_maps(depth),
            Self::Not => nested_nots(depth),
            Self::Binder | Self::SpatialBinder => nested_binders(depth),
            Self::Send => nested_send_chain(depth),
            Self::PathMap | Self::SpatialPathMap => nested_pathmap_chain(depth),
            Self::SpatialList => nested_list(depth),
        };
        let pattern = match self {
            Self::SpatialList => Some(nested_list_pattern(depth)),
            Self::SpatialBinder => Some(nested_binders(depth)),
            Self::SpatialPathMap => Some(nested_pathmap_match_pattern(depth)),
            _ => None,
        };
        let env = (self == Self::Binder).then(|| Env::new().put(nested_list(depth)));
        Fixture {
            target: Some(target),
            pattern,
            env,
        }
    }

    fn depth(self, par: &Par) -> usize {
        match self {
            Self::Set => eset_depth(par),
            Self::Map => emap_depth(par),
            Self::Not => enot_depth(par),
            Self::Binder | Self::SpatialBinder => binder_depth(par),
            Self::Send => send_chain_depth(par),
            Self::PathMap | Self::SpatialPathMap => pathmap_chain_depth(par),
            Self::SpatialList => par_depth(par),
        }
    }

    fn witness(self, fixture: &Fixture, depth: usize) {
        assert_eq!(self.depth(fixture.target.as_ref().unwrap()), depth);
        if let Some(pattern) = &fixture.pattern {
            let observed = if self == Self::SpatialList {
                planned_deep_fixture::list_pattern_depth(pattern)
            } else {
                self.depth(pattern)
            };
            assert_eq!(observed, depth);
        }
        if let Some(env) = &fixture.env {
            assert_eq!(env.env_map.len(), 1);
            assert_eq!(par_depth(env.env_map.values().next().unwrap()), depth);
        }
    }

    fn operate(self, fixture: &mut Fixture) -> Output {
        match self {
            Self::Set | Self::Map => {
                Output::Sorted(ParSortMatcher::sort_match(fixture.target.as_ref().unwrap()))
            }
            Self::Not => Output::Par(
                rho_pure_eval::eval_with(
                    fixture.target.as_ref().unwrap(),
                    &Env::new(),
                    &rho_pure_eval::NoSpatialMatch,
                )
                .expect("ENot evaluation"),
            ),
            Self::Binder => Output::Par(
                substitute_instance()
                    .substitute_no_sort(
                        fixture.target.take().unwrap(),
                        0,
                        fixture.env.as_ref().unwrap(),
                    )
                    .expect("binder substitution"),
            ),
            Self::Send | Self::PathMap => Output::Par(fixture.target.as_ref().unwrap().clone()),
            Self::SpatialList | Self::SpatialBinder | Self::SpatialPathMap => {
                let mut context = SpatialMatcherContext::new();
                assert!(context
                    .spatial_match_result(
                        fixture.target.take().unwrap(),
                        fixture.pattern.take().unwrap(),
                    )
                    .is_some());
                Output::Matched(context)
            }
        }
    }

    fn result_witness(self, output: &Output, depth: usize) {
        match output {
            Output::None => {}
            Output::Par(par) if self == Self::Not => {
                assert_eq!(
                    par,
                    &models::rust::utils::new_gbool_par(depth % 2 == 0, Vec::new(), false)
                );
            }
            Output::Par(par) => assert_eq!(self.depth(par), depth),
            Output::Sorted(sorted) => assert_eq!(self.depth(&sorted.term), depth),
            Output::Matched(context) if self == Self::SpatialBinder => {
                assert!(context.free_map.is_empty());
            }
            Output::Matched(context) => {
                assert_eq!(context.free_map.len(), 1);
                assert_eq!(
                    context.free_map.get(&0),
                    Some(&new_gint_par(0, Vec::new(), false))
                );
            }
        }
    }
}

#[derive(Debug)]
struct Phase {
    name: &'static str,
    executed: bool,
    nanos: u128,
    allocations: usize,
    requested_bytes: usize,
}

fn measure<T>(name: &'static str, action: impl FnOnce() -> T) -> (T, Phase) {
    let ((value, nanos), allocations, requested_bytes) = counting_alloc::measure(|| {
        let start = Instant::now();
        let value = black_box(action());
        (value, start.elapsed().as_nanos())
    });
    (value, Phase {
        name,
        executed: true,
        nanos,
        allocations,
        requested_bytes,
    })
}

fn run(family: Family, depth: usize, operation: bool, mut record: impl FnMut(Phase)) {
    let (mut fixture, phase) = measure("construction", || family.construct(depth));
    record(phase);
    let (_, phase) = measure("input_witness", || family.witness(&fixture, depth));
    record(phase);
    let (output, phase) = if operation {
        measure("operation", || family.operate(&mut fixture))
    } else {
        (Output::None, Phase {
            name: "operation",
            executed: false,
            nanos: 0,
            allocations: 0,
            requested_bytes: 0,
        })
    };
    record(phase);
    let phase = if operation {
        measure("result_witness", || family.result_witness(&output, depth)).1
    } else {
        Phase {
            name: "result_witness",
            executed: false,
            nanos: 0,
            allocations: 0,
            requested_bytes: 0,
        }
    };
    record(phase);
    let (_, phase) = measure("teardown", || drop((output, fixture)));
    record(phase);
}

#[test]
#[ignore = "fresh-process phase probe; requires FIXTURE_FAMILY, FIXTURE_ARM and FIXTURE_DEPTH"]
fn phase_child() {
    let label = std::env::var("FIXTURE_FAMILY").unwrap();
    let family = FAMILIES.iter().find(|(name, _)| *name == label).unwrap().1;
    let arm = std::env::var("FIXTURE_ARM").unwrap();
    let operation = match arm.as_str() {
        "fixture" => false,
        "operation" => true,
        _ => panic!("unknown fixture arm"),
    };
    let depth = std::env::var("FIXTURE_DEPTH").unwrap().parse().unwrap();
    std::thread::Builder::new()
        .stack_size(1_048_576)
        .spawn(move || {
            run(family, depth, operation, |phase| {
                println!(
                    "FIXTURE_SAMPLE\t{label}\t{arm}\t{depth}\t{}\t{}\t{}\t{}\t{}",
                    phase.name,
                    u8::from(phase.executed),
                    phase.nanos,
                    phase.allocations,
                    phase.requested_bytes
                );
                std::io::stdout().flush().unwrap();
            });
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn phase_probe_covers_exact_fixtures_and_real_operations() {
    for (_, family) in FAMILIES {
        for depth in [0, 1, 7, 32] {
            for operation in [false, true] {
                let mut phases = Vec::new();
                run(family, depth, operation, |phase| phases.push(phase));
                assert_eq!(phases.iter().map(|phase| phase.name).collect::<Vec<_>>(), [
                    "construction",
                    "input_witness",
                    "operation",
                    "result_witness",
                    "teardown"
                ]);
                assert!(phases[0].allocations > 0);
                assert!(phases[0].requested_bytes > 0);
                if !operation {
                    assert!(!phases[2].executed);
                    assert!(!phases[3].executed);
                    assert_eq!(phases[2].nanos, 0);
                    assert_eq!(phases[3].nanos, 0);
                    assert_eq!(phases[2].allocations, 0);
                    assert_eq!(phases[2].requested_bytes, 0);
                }
            }
        }
    }
}

#[test]
fn phase_counter_includes_zeroed_allocations() {
    let layout = std::alloc::Layout::from_size_align(64, 8).unwrap();
    let (_, phase) = measure("zeroed", || unsafe {
        let memory = std::alloc::alloc_zeroed(layout);
        assert!(!memory.is_null());
        black_box(memory);
        std::alloc::dealloc(memory, layout);
    });
    assert_eq!(phase.allocations, 1);
    assert_eq!(phase.requested_bytes, 64);
}
