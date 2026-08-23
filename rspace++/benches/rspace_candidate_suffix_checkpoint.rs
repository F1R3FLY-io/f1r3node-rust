//! Paired release experiment for retained candidate-product continuations.
//!
//! The treatment invokes the production CoW suffix-checkpoint implementation.
//! The control is the feature-gated frozen read-zipper clone implementation in
//! `tests/data_index`; neither is reconstructed in this benchmark binary.

use std::env;
use std::hint::black_box;
use std::time::Instant;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rspace_plus_plus::rspace::data_index::candidate_checkpoint_experiment::{
    CandidateCheckpointExperiment, ExperimentArm, ExperimentOutcome, ExperimentQuery,
    ExperimentShape,
};

const SCHEMA: &str = "candidate-suffix-checkpoint-v2";
const SEED: u64 = 0x4131_5990_2026_0823;
const DEPTHS: [usize; 5] = [256, 512, 1024, 2048, 4096];
const SHAPES: [ExperimentShape; 2] = [ExperimentShape::Compressed, ExperimentShape::Dense64];
const QUERIES: [ExperimentQuery; 5] = [
    ExperimentQuery::Exact,
    ExperimentQuery::Wildcard,
    ExperimentQuery::RepeatedCapture,
    ExperimentQuery::NestedMatch,
    ExperimentQuery::AcceptsRemainder,
];
const WARMUPS: usize = 3;
const RETAINED_PAIRS: usize = 30;

struct Observation {
    shape: &'static str,
    query: &'static str,
    depth: usize,
    pair: usize,
    order: &'static str,
    arm: &'static str,
    batch_iterations: usize,
    elapsed_ns: u128,
    result_count: usize,
    result_digest: String,
}

fn main() {
    if env::var_os("CANDIDATE_CHECKPOINT_PROFILE_ARM").is_some() {
        run_profile();
    } else {
        run_matrix();
    }
}

fn run_matrix() {
    let mut rng = StdRng::seed_from_u64(SEED);
    let mut checks = Vec::new();
    let mut observations =
        Vec::with_capacity(DEPTHS.len() * SHAPES.len() * QUERIES.len() * RETAINED_PAIRS * 2);

    for shape in SHAPES {
        for query in QUERIES {
            for depth in DEPTHS {
                let fixture = CandidateCheckpointExperiment::new(shape, query, depth);
                let audit = fixture.audit();
                checks.push(format!(
                    "CHECK\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    shape.label(),
                    query.label(),
                    depth,
                    audit.logical_structure_bytes,
                    audit.expected.count(),
                    audit.expected.digest_hex(),
                    audit.legacy_metrics.visited_frames,
                    audit.legacy_metrics.zipper_pairs_cloned,
                    audit.legacy_metrics.total_prefix_bytes_cloned,
                    audit.legacy_metrics.maximum_prefix_bytes_cloned,
                ));

                let batch_iterations = batch_iterations(depth, shape);
                for warmup in 0..WARMUPS {
                    let legacy_first = (warmup + depth) % 2 == 0;
                    run_pair(&fixture, batch_iterations, legacy_first);
                }

                for pair in 0..RETAINED_PAIRS {
                    let legacy_first = rng.gen::<bool>();
                    let order = if legacy_first { "AB" } else { "BA" };
                    let arms = if legacy_first {
                        [ExperimentArm::LegacyClone, ExperimentArm::CowCheckpoint]
                    } else {
                        [ExperimentArm::CowCheckpoint, ExperimentArm::LegacyClone]
                    };
                    for arm in arms {
                        let (elapsed_ns, result) = measure(&fixture, arm, batch_iterations);
                        observations.push(Observation {
                            shape: shape.label(),
                            query: query.label(),
                            depth,
                            pair,
                            order,
                            arm: arm.label(),
                            batch_iterations,
                            elapsed_ns,
                            result_count: result.count(),
                            result_digest: result.digest_hex(),
                        });
                    }
                }
                eprintln!(
                    "measured {} {} depth {} ({} retained pairs)",
                    shape.label(),
                    query.label(),
                    depth,
                    RETAINED_PAIRS
                );
            }
        }
    }

    let expected = DEPTHS.len() * SHAPES.len() * QUERIES.len() * RETAINED_PAIRS * 2;
    assert_eq!(observations.len(), expected, "raw observation count drifted");
    println!(
        concat!("META\t{}\tseed={}\twarmups={}", "\tretained_pairs={}\trows={}"),
        SCHEMA, SEED, WARMUPS, RETAINED_PAIRS, expected
    );
    println!(concat!(
        "CHECK_HEADER\tschema\tshape\tquery\tdepth\tlogical_structure_bytes",
        "\tresult_count\tresult_digest\tlegacy_visited_frames",
        "\tlegacy_zipper_pairs_cloned\tlegacy_total_prefix_bytes_cloned",
        "\tlegacy_maximum_prefix_bytes_cloned"
    ));
    for check in checks {
        println!("{check}");
    }
    println!(concat!(
        "OBS_HEADER\tschema\tshape\tquery\tdepth\tpair\torder\tarm",
        "\tbatch_iterations\telapsed_ns\tresult_count\tresult_digest"
    ));
    for observation in observations {
        println!(
            "OBS\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            observation.shape,
            observation.query,
            observation.depth,
            observation.pair,
            observation.order,
            observation.arm,
            observation.batch_iterations,
            observation.elapsed_ns,
            observation.result_count,
            observation.result_digest,
        );
    }
}

fn run_profile() {
    let arm = parse_arm(&required("CANDIDATE_CHECKPOINT_PROFILE_ARM"));
    let shape = parse_shape(&required("CANDIDATE_CHECKPOINT_PROFILE_SHAPE"));
    let query = parse_query(&required("CANDIDATE_CHECKPOINT_PROFILE_QUERY"));
    let depth = required("CANDIDATE_CHECKPOINT_PROFILE_DEPTH")
        .parse::<usize>()
        .expect("CANDIDATE_CHECKPOINT_PROFILE_DEPTH must be a positive usize");
    let iterations = env::var("CANDIDATE_CHECKPOINT_PROFILE_ITERATIONS")
        .unwrap_or_else(|_| "100".to_owned())
        .parse::<usize>()
        .expect("CANDIDATE_CHECKPOINT_PROFILE_ITERATIONS must be a positive usize");
    assert!(iterations > 0);

    let fixture = CandidateCheckpointExperiment::new(shape, query, depth);
    let audit = fixture.audit();
    let (elapsed_ns, result) = measure(&fixture, arm, iterations);
    assert_eq!(result, audit.expected);
    println!(
        "PROFILE\t{SCHEMA}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        shape.label(),
        query.label(),
        depth,
        arm.label(),
        iterations,
        elapsed_ns,
        result.digest_hex(),
    );
}

fn run_pair(fixture: &CandidateCheckpointExperiment, batch_iterations: usize, legacy_first: bool) {
    let arms = if legacy_first {
        [ExperimentArm::LegacyClone, ExperimentArm::CowCheckpoint]
    } else {
        [ExperimentArm::CowCheckpoint, ExperimentArm::LegacyClone]
    };
    for arm in arms {
        black_box(measure(fixture, arm, batch_iterations));
    }
}

#[inline(never)]
fn measure(
    fixture: &CandidateCheckpointExperiment,
    arm: ExperimentArm,
    batch_iterations: usize,
) -> (u128, ExperimentOutcome) {
    let fixture = black_box(fixture);
    let mut outcomes = Vec::with_capacity(batch_iterations);
    let started = Instant::now();
    for _ in 0..batch_iterations {
        outcomes.push(black_box(fixture.run(black_box(arm))));
    }
    let elapsed_ns = started.elapsed().as_nanos();
    black_box(&outcomes);
    let first = outcomes
        .first()
        .expect("the measurement batch is nonempty")
        .clone();
    assert!(outcomes.iter().all(|outcome| outcome == &first));
    (elapsed_ns, first)
}

const fn batch_iterations(depth: usize, shape: ExperimentShape) -> usize {
    match (depth, shape) {
        (..=512, ExperimentShape::Compressed) => 4,
        (..=1024, ExperimentShape::Compressed) => 2,
        _ => 1,
    }
}

fn required(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("{name} is required in profile mode"))
}

fn parse_arm(value: &str) -> ExperimentArm {
    match value {
        "legacy_clone" => ExperimentArm::LegacyClone,
        "cow_checkpoint" => ExperimentArm::CowCheckpoint,
        _ => panic!("unknown checkpoint experiment arm {value:?}"),
    }
}

fn parse_shape(value: &str) -> ExperimentShape {
    match value {
        "compressed" => ExperimentShape::Compressed,
        "dense64" => ExperimentShape::Dense64,
        _ => panic!("unknown checkpoint experiment shape {value:?}"),
    }
}

fn parse_query(value: &str) -> ExperimentQuery {
    match value {
        "exact" => ExperimentQuery::Exact,
        "wildcard" => ExperimentQuery::Wildcard,
        "repeated_capture" => ExperimentQuery::RepeatedCapture,
        "nested_match" => ExperimentQuery::NestedMatch,
        "accepts_remainder" => ExperimentQuery::AcceptsRemainder,
        _ => panic!("unknown checkpoint experiment query {value:?}"),
    }
}
