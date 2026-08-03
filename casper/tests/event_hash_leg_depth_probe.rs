//! Native-stack depth gate for the three consensus event-hash serialization legs.
//!
//! Produce and consume hashes include the bincode bytes of datums, bind
//! patterns, and continuations. All three roots now use the generated
//! explicit-worklist encoder unconditionally. The former intern-store splice
//! branch and its `contains_par` dispatch traversal no longer exist.
//!
//! This probe crosses every root with two term shapes:
//!
//! - a deeply nested list without EPathMap;
//! - a map-mode EPathMap whose `PathMap<Par>` value is the deeply nested term.
//!
//! The second shape is load-bearing: it proves that direct EPM1 snapshot
//! emission and nested map-value encoding remain on the same stack-safe PDA.
//! Each probe point runs in a child process because native stack overflow is
//! aborting rather than unwindable. Fixture construction occurs before the
//! explicitly sized subject thread and is iterative, so the reported stack is
//! the event-hash traversal alone.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, EList, EPathMap, Expr, ListParWithRandom, Par, ParWithRandom, TaggedContinuation,
};
use models::rust::event_hash_bytes::{
    event_hash_bytes_bind_pattern, event_hash_bytes_list_par_with_random,
    event_hash_bytes_tagged_continuation,
};
use models::rust::utils::new_gint_par;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leg {
    Datum,
    Pattern,
    Continuation,
    DatumEPathMap,
    PatternEPathMap,
    ContinuationEPathMap,
}

impl Leg {
    fn tag(self) -> &'static str {
        match self {
            Leg::Datum => "datum",
            Leg::Pattern => "pattern",
            Leg::Continuation => "continuation",
            Leg::DatumEPathMap => "datum-epathmap",
            Leg::PatternEPathMap => "pattern-epathmap",
            Leg::ContinuationEPathMap => "continuation-epathmap",
        }
    }

    fn from_tag(tag: &str) -> Self {
        match tag {
            "datum" => Leg::Datum,
            "pattern" => Leg::Pattern,
            "continuation" => Leg::Continuation,
            "datum-epathmap" => Leg::DatumEPathMap,
            "pattern-epathmap" => Leg::PatternEPathMap,
            "continuation-epathmap" => Leg::ContinuationEPathMap,
            other => unreachable!("event_hash_leg_depth_probe: unknown leg {other:?}"),
        }
    }

    fn carries_epathmap(self) -> bool {
        matches!(
            self,
            Leg::DatumEPathMap | Leg::PatternEPathMap | Leg::ContinuationEPathMap
        )
    }
}

const LEGS: [Leg; 6] = [
    Leg::Datum,
    Leg::Pattern,
    Leg::Continuation,
    Leg::DatumEPathMap,
    Leg::PatternEPathMap,
    Leg::ContinuationEPathMap,
];

const RESOLUTION: usize = 4096;
const LADDER_LO: usize = 256;
const LADDER_HI: usize = 4_096;
const ZERO_SLOPE_TOLERANCE: usize = 4 * RESOLUTION;

fn expr_par(instance: ExprInstance) -> Par {
    models::par_from_default! {
        exprs: vec![Expr {
            expr_instance: Some(instance),
        }],
        ..Default::default()
    }
}

fn elist(ps: Vec<Par>) -> Par {
    expr_par(ExprInstance::EListBody(EList {
        ps,
        locally_free: vec![],
        connective_used: false,
        remainder: None,
    }))
}

fn nested_list(depth: usize) -> Par {
    let mut par = new_gint_par(0, vec![], false);
    for _ in 0..depth {
        par = elist(vec![par]);
    }
    par
}

fn map_with_nested_value(depth: usize) -> Par {
    let map = EPathMap::new_map(
        [(new_gint_par(1, vec![], false), nested_list(depth))],
        vec![],
        false,
        None,
    );
    expr_par(ExprInstance::EPathmapBody(map))
}

fn datum_of(par: Par) -> ListParWithRandom {
    ListParWithRandom {
        pars: vec![par],
        random_state: vec![0xAB; 32],
    }
}

fn pattern_of(par: Par) -> BindPattern {
    BindPattern {
        patterns: vec![par],
        remainder: None,
        free_count: 0,
    }
}

fn continuation_of(par: Par) -> TaggedContinuation {
    TaggedContinuation {
        guard: None,
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(par),
            random_state: vec![0xCD; 32],
        })),
    }
}

enum Fixture {
    Datum(ListParWithRandom),
    Pattern(BindPattern),
    Continuation(TaggedContinuation),
}

fn build_fixture(leg: Leg, depth: usize) -> Fixture {
    let par = if leg.carries_epathmap() {
        map_with_nested_value(depth)
    } else {
        nested_list(depth)
    };

    match leg {
        Leg::Datum | Leg::DatumEPathMap => Fixture::Datum(datum_of(par)),
        Leg::Pattern | Leg::PatternEPathMap => Fixture::Pattern(pattern_of(par)),
        Leg::Continuation | Leg::ContinuationEPathMap => {
            Fixture::Continuation(continuation_of(par))
        }
    }
}

fn run_leg(leg: Leg, depth: usize, fixture: Fixture) {
    let bytes = match (leg, fixture) {
        (Leg::Datum | Leg::DatumEPathMap, Fixture::Datum(value)) => {
            let bytes = event_hash_bytes_list_par_with_random(&value);
            std::mem::forget(value);
            bytes
        }
        (Leg::Pattern | Leg::PatternEPathMap, Fixture::Pattern(value)) => {
            let bytes = event_hash_bytes_bind_pattern(&value);
            std::mem::forget(value);
            bytes
        }
        (Leg::Continuation | Leg::ContinuationEPathMap, Fixture::Continuation(value)) => {
            let bytes = event_hash_bytes_tagged_continuation(&value);
            std::mem::forget(value);
            bytes
        }
        (wrong, _) => unreachable!(
            "event_hash_leg_depth_probe: leg {} received the wrong root fixture",
            wrong.tag()
        ),
    };

    assert!(
        bytes.len() > depth,
        "VACUOUS PROBE: leg {} at depth {depth} produced only {} bytes",
        leg.tag(),
        bytes.len()
    );
    std::mem::forget(bytes);
}

#[test]
#[ignore = "child process of the event-hash leg probe; driven via EVENT_HASH_DEPTH"]
fn event_hash_leg_child() {
    let Ok(depth) = std::env::var("EVENT_HASH_DEPTH") else {
        return;
    };
    let depth: usize = depth.parse().expect("EVENT_HASH_DEPTH must be an integer");
    let stack: usize = std::env::var("EVENT_HASH_STACK")
        .expect("EVENT_HASH_STACK must accompany EVENT_HASH_DEPTH")
        .parse()
        .expect("EVENT_HASH_STACK must be an integer");
    let leg = Leg::from_tag(
        &std::env::var("EVENT_HASH_LEG").expect("EVENT_HASH_LEG must accompany EVENT_HASH_DEPTH"),
    );

    let fixture = build_fixture(leg, depth);
    std::thread::Builder::new()
        .stack_size(stack)
        .name("event-hash-leg".to_string())
        .spawn(move || run_leg(leg, depth, fixture))
        .expect("event_hash_leg_depth_probe: failed to spawn")
        .join()
        .expect("event_hash_leg_depth_probe: subject panicked");
}

fn leg_survives(leg: Leg, depth: usize, stack: usize) -> bool {
    let exe = std::env::current_exe().expect("event_hash_leg_depth_probe: current_exe");
    std::process::Command::new(exe)
        .args(["--ignored", "--exact", "event_hash_leg_child"])
        .env("EVENT_HASH_LEG", leg.tag())
        .env("EVENT_HASH_DEPTH", depth.to_string())
        .env("EVENT_HASH_STACK", stack.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("event_hash_leg_depth_probe: failed to run child")
        .success()
}

fn min_stack_for(leg: Leg, depth: usize) -> usize {
    const CEILING: usize = 64 * 1024 * 1024;
    let mut hi = 16 * 1024;
    while hi <= CEILING && !leg_survives(leg, depth, hi) {
        hi *= 2;
    }
    assert!(
        hi <= CEILING,
        "leg {} depth {depth} needs more than {CEILING} bytes of native stack",
        leg.tag()
    );

    let mut lo = hi / 2;
    while hi - lo > RESOLUTION {
        let mid = lo + (hi - lo) / 2;
        if leg_survives(leg, depth, mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

#[test]
fn event_hash_legs_are_depth_independent() {
    println!("\n  leg                       min stack @ {LADDER_LO}   @ {LADDER_HI}     B/level");
    println!("  ─────────────────────────  ───────────────  ───────────────  ─────────");

    for leg in LEGS {
        let lo = min_stack_for(leg, LADDER_LO);
        let hi = min_stack_for(leg, LADDER_HI);
        let slope = (hi.saturating_sub(lo)) as f64 / (LADDER_HI - LADDER_LO) as f64;
        println!("  {:<25}  {lo:>15}  {hi:>15}  {slope:>9.1}", leg.tag());
        assert!(
            hi <= lo + ZERO_SLOPE_TOLERANCE,
            "event-hash leg {} grew from {lo} to {hi} bytes over depth \
             {LADDER_LO}..{LADDER_HI}; the generated encoder is no longer stack-safe",
            leg.tag()
        );
    }
}

#[test]
fn event_hash_legs_survive_deep_terms_on_one_mebibyte() {
    const DEEP: usize = 1 << 16;
    const STACK: usize = 1024 * 1024;

    for leg in LEGS {
        assert!(
            leg_survives(leg, DEEP, STACK),
            "event-hash leg {} did not survive depth {DEEP} on a one-MiB stack",
            leg.tag()
        );
    }
}

#[test]
fn epathmap_event_hash_legs_are_byte_identical_to_legacy_bincode() {
    let par = map_with_nested_value(16);

    let datum = datum_of(par.clone());
    assert_eq!(
        event_hash_bytes_list_par_with_random(&datum),
        bincode::serialize(&datum).expect("legacy datum oracle must encode")
    );

    let pattern = pattern_of(par.clone());
    assert_eq!(
        event_hash_bytes_bind_pattern(&pattern),
        bincode::serialize(&pattern).expect("legacy pattern oracle must encode")
    );

    let continuation = continuation_of(par);
    assert_eq!(
        event_hash_bytes_tagged_continuation(&continuation),
        bincode::serialize(&continuation).expect("legacy continuation oracle must encode")
    );
}
