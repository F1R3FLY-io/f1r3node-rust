//! Exact paired runner for pgmcp experiment 303.
//!
//! This is deliberately not a Criterion benchmark. The locked protocol requires
//! exactly three warm-up pairs followed by thirty retained pairs in alternating
//! AB/BA order. Criterion's adaptive batching cannot provide that experimental
//! unit. The control below is a frozen, name-only adaptation of the production
//! traversal at `c847838127231fcfcd9a547a3591c86313a1626c`; the SHA-256 of that
//! complete historical source file is recorded in every output file.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::hint::black_box;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{Bundle, Expr, If, Match, New, Par, Receive, Send};
use models::rust::par_construction::ParConstruction;
use prost::Message;
use rholang::rust::interpreter::accounting::delta_sigma::{
    demand, demand_by_sig, DemandEntry, SigKey,
};
use rholang::rust::interpreter::accounting::Sig;

const EXPERIMENT_ID: u64 = 303;
const SEED: u64 = 0x0041_31D3_5909;
const FROZEN_CONTROL_GIT: &str = "c847838127231fcfcd9a547a3591c86313a1626c";
const FROZEN_SOURCE_SHA256: &str =
    "7c4d4bf64e4705d27b55e3d1484464d13ce2b2ccc52e37242bde6be54f13aacf";
const ENVELOPE_KEY: SigKey = [0xee; 32];
const FIRST_LANE: SigKey = [0xa1; 32];
const SECOND_LANE: SigKey = [0xb2; 32];
const CALLBACK_INITIAL_STATE: u64 = 0xcbf2_9ce4_8422_2325;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Locked,
    Preflight,
    Profile,
    Smoke,
}

impl Mode {
    const fn name(self) -> &'static str {
        match self {
            Self::Locked => "locked",
            Self::Preflight => "preflight",
            Self::Profile => "profile",
            Self::Smoke => "smoke",
        }
    }

    const fn protocol(self) -> Protocol {
        match self {
            Self::Locked => Protocol {
                depth: 1_024,
                warmup_pairs: 3,
                retained_pairs: 30,
            },
            Self::Preflight => Protocol {
                depth: 512,
                warmup_pairs: 0,
                retained_pairs: 1,
            },
            Self::Profile => panic!("profile mode supplies an explicit protocol"),
            Self::Smoke => Protocol {
                depth: 16,
                warmup_pairs: 1,
                retained_pairs: 2,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Protocol {
    depth: usize,
    warmup_pairs: usize,
    retained_pairs: usize,
}

#[derive(Debug)]
struct Config {
    mode: Mode,
    output: PathBuf,
    treatment_ref: String,
    expected_cpu: Option<usize>,
    profile: Option<ProfileConfig>,
}

#[derive(Clone, Copy, Debug)]
struct ProfileConfig {
    arm: Arm,
    depth: usize,
    iterations: usize,
}

fn usage() -> ! {
    eprintln!(
        "{}",
        concat!(
            "usage: accounting_demand_paired --mode locked|preflight|smoke ",
            "--output PATH --treatment-ref GIT_REF [--cpu LOGICAL_CPU]\n",
            "profiling: --mode profile --profile-arm control|treatment ",
            "--profile-depth N --profile-iterations N --output PATH ",
            "--treatment-ref GIT_REF --cpu LOGICAL_CPU"
        )
    );
    std::process::exit(2);
}

fn parse_config() -> Config {
    let mut args = std::env::args().skip(1);
    let mut mode = None;
    let mut output = None;
    let mut treatment_ref = None;
    let mut expected_cpu = None;
    let mut profile_arm = None;
    let mut profile_depth = None;
    let mut profile_iterations = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mode" => {
                mode = Some(match args.next().as_deref() {
                    Some("locked") => Mode::Locked,
                    Some("preflight") => Mode::Preflight,
                    Some("profile") => Mode::Profile,
                    Some("smoke") => Mode::Smoke,
                    _ => usage(),
                });
            }
            "--output" => output = args.next().map(PathBuf::from),
            "--treatment-ref" => treatment_ref = args.next(),
            "--profile-arm" => {
                profile_arm = Some(match args.next().as_deref() {
                    Some("control") => Arm::Control,
                    Some("treatment") => Arm::Treatment,
                    _ => usage(),
                });
            }
            "--profile-depth" => {
                profile_depth = Some(
                    args.next()
                        .and_then(|raw| raw.parse::<usize>().ok())
                        .filter(|&value| value != 0)
                        .unwrap_or_else(|| usage()),
                );
            }
            "--profile-iterations" => {
                profile_iterations = Some(
                    args.next()
                        .and_then(|raw| raw.parse::<usize>().ok())
                        .filter(|&value| value != 0)
                        .unwrap_or_else(|| usage()),
                );
            }
            "--cpu" => {
                expected_cpu = Some(
                    args.next()
                        .and_then(|raw| raw.parse::<usize>().ok())
                        .unwrap_or_else(|| usage()),
                );
            }
            // Cargo passes this argument when invoking a harness-free bench.
            "--bench" => {}
            _ => usage(),
        }
    }

    let mode = mode.unwrap_or_else(|| usage());
    let profile = match mode {
        Mode::Profile => Some(ProfileConfig {
            arm: profile_arm.unwrap_or_else(|| usage()),
            depth: profile_depth.unwrap_or_else(|| usage()),
            iterations: profile_iterations.unwrap_or_else(|| usage()),
        }),
        _ if profile_arm.is_none() && profile_depth.is_none() && profile_iterations.is_none() => {
            None
        }
        _ => usage(),
    };
    let config = Config {
        mode,
        output: output.unwrap_or_else(|| usage()),
        treatment_ref: treatment_ref.unwrap_or_else(|| usage()),
        expected_cpu,
        profile,
    };
    if matches!(config.mode, Mode::Locked | Mode::Profile) && config.expected_cpu.is_none() {
        eprintln!("locked and profile modes require --cpu and a one-CPU affinity mask");
        std::process::exit(2);
    }
    config
}

fn affinity_cpus() -> Vec<usize> {
    let mut set = unsafe { std::mem::zeroed::<libc::cpu_set_t>() };
    let rc = unsafe {
        libc::sched_getaffinity(
            0,
            std::mem::size_of::<libc::cpu_set_t>(),
            std::ptr::addr_of_mut!(set),
        )
    };
    assert_eq!(
        rc,
        0,
        "sched_getaffinity failed: {}",
        std::io::Error::last_os_error()
    );
    (0..libc::CPU_SETSIZE as usize)
        .filter(|&cpu| unsafe { libc::CPU_ISSET(cpu, &set) })
        .collect()
}

fn current_cpu() -> usize {
    let cpu = unsafe { libc::sched_getcpu() };
    assert!(
        cpu >= 0,
        "sched_getcpu failed: {}",
        std::io::Error::last_os_error()
    );
    cpu as usize
}

fn byte_channel(id: u8) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(vec![id; 32])),
    }])
}

fn deep_new_chain(depth: usize) -> (Par, Vec<u8>, Vec<u8>) {
    let first_channel = byte_channel(0x31);
    let second_channel = byte_channel(0x72);
    let first_bytes = first_channel.encode_to_vec();
    let second_bytes = second_channel.encode_to_vec();

    let mut construction = ParConstruction::new();
    let first_channel = construction
        .import_par(&first_channel)
        .expect("planned first channel");
    let second_channel = construction
        .import_par(&second_channel)
        .expect("planned second channel");
    let send_shell = Send {
        chan: Some(Par::default()),
        data: Vec::new(),
        persistent: false,
        locally_free: Vec::new(),
        connective_used: false,
        cost_region: None,
    };
    let first_send = construction
        .send_component(&send_shell, &[first_channel])
        .expect("planned first Send");
    let second_send = construction
        .send_component(&send_shell, &[second_channel])
        .expect("planned second Send");
    let first_send = construction
        .singleton_component(first_send, Vec::new(), false)
        .expect("planned first Send Par");
    let second_send = construction
        .singleton_component(second_send, Vec::new(), false)
        .expect("planned second Send Par");
    let mut planned = construction
        .parallel(first_send, second_send)
        .expect("planned terminal parallel sends");
    let new_shell = New {
        bind_count: 1,
        p: Some(Par::default()),
        uri: Vec::new(),
        injections: BTreeMap::new(),
        locally_free: Vec::new(),
    };
    for _ in 0..depth {
        let new = construction
            .new_component(&new_shell, &[planned])
            .expect("planned New");
        planned = construction
            .singleton_component(new, Vec::new(), false)
            .expect("planned New Par");
    }
    (
        construction.finish(planned).expect("finish planned Par"),
        first_bytes,
        second_bytes,
    )
}

// Frozen implementation from c847838. Names have a `legacy_` prefix so this
// benchmark can link the current production implementation in the same binary.
enum LegacyDemandWalkEvent {
    Comm(Option<Par>),
    Unknown,
}

enum LegacyDemandWalkTask {
    Visit(Par),
    Comm(Option<Par>),
    ScanUnknown(Par),
}

fn legacy_walk_demand_events(root: &Par, mut visit: impl FnMut(LegacyDemandWalkEvent)) {
    let mut tasks = vec![LegacyDemandWalkTask::Visit(root.clone())];
    while let Some(task) = tasks.pop() {
        match task {
            LegacyDemandWalkTask::Comm(channel) => {
                visit(LegacyDemandWalkEvent::Comm(channel));
            }
            LegacyDemandWalkTask::ScanUnknown(par) => {
                for expr in par.decode_occurrences::<Expr>() {
                    if let Some(ExprInstance::EVarBody(evar)) = &expr.expr_instance {
                        if let Some(var) = &evar.v {
                            if matches!(
                                &var.var_instance,
                                Some(VarInstance::BoundVar(_)) | Some(VarInstance::FreeVar(_))
                            ) {
                                visit(LegacyDemandWalkEvent::Unknown);
                            }
                        }
                    }
                }
            }
            LegacyDemandWalkTask::Visit(mut par) => {
                tasks.push(LegacyDemandWalkTask::ScanUnknown(par.clone()));

                for bundle in par.take_occurrences::<Bundle>().into_iter().rev() {
                    if let Some(body) = bundle.body {
                        tasks.push(LegacyDemandWalkTask::Visit(body));
                    }
                }
                for conditional in par.take_occurrences::<If>().into_iter().rev() {
                    if let Some(if_false) = conditional.if_false {
                        tasks.push(LegacyDemandWalkTask::Visit(if_false));
                    }
                    if let Some(if_true) = conditional.if_true {
                        tasks.push(LegacyDemandWalkTask::Visit(if_true));
                    }
                }
                for mat in par.take_occurrences::<Match>().into_iter().rev() {
                    for case in mat.cases.into_iter().rev() {
                        if let Some(source) = case.source {
                            tasks.push(LegacyDemandWalkTask::Visit(source));
                        }
                    }
                }
                for new in par.take_occurrences::<New>().into_iter().rev() {
                    if let Some(body) = new.p {
                        tasks.push(LegacyDemandWalkTask::Visit(body));
                    }
                }
                for receive in par.take_occurrences::<Receive>().into_iter().rev() {
                    if let Some(body) = receive.body {
                        tasks.push(LegacyDemandWalkTask::Visit(body));
                    }
                    let channel = receive
                        .binds
                        .into_iter()
                        .next()
                        .and_then(|bind| bind.source);
                    tasks.push(LegacyDemandWalkTask::Comm(channel));
                }
                for send in par.take_occurrences::<Send>().into_iter().rev() {
                    tasks.push(LegacyDemandWalkTask::Comm(send.chan));
                }
            }
        }
    }
}

fn add_entry(left: DemandEntry, right: DemandEntry) -> DemandEntry {
    DemandEntry {
        known_lower_bound: left
            .known_lower_bound
            .saturating_add(right.known_lower_bound),
        unknown: left.unknown || right.unknown,
    }
}

fn one_comm() -> DemandEntry {
    DemandEntry {
        known_lower_bound: 1,
        unknown: false,
    }
}

fn legacy_demand_par(par: &Par) -> DemandEntry {
    let mut acc = DemandEntry::ZERO;
    legacy_walk_demand_events(par, |event| match event {
        LegacyDemandWalkEvent::Comm(_) => acc = add_entry(acc, one_comm()),
        LegacyDemandWalkEvent::Unknown => acc.unknown = true,
    });
    acc
}

fn legacy_bump_lane(acc: &mut BTreeMap<SigKey, DemandEntry>, lane: SigKey, entry: DemandEntry) {
    let slot = acc.entry(lane).or_insert(DemandEntry::ZERO);
    *slot = add_entry(*slot, entry);
}

fn legacy_demand_by_sig(
    par: &Par,
    envelope_key: SigKey,
    region_sig: &dyn Fn(&Par) -> Option<SigKey>,
) -> BTreeMap<SigKey, DemandEntry> {
    let mut acc = BTreeMap::new();
    legacy_walk_demand_events(par, |event| match event {
        LegacyDemandWalkEvent::Comm(channel) => {
            let lane = channel
                .as_ref()
                .and_then(region_sig)
                .unwrap_or(envelope_key);
            legacy_bump_lane(&mut acc, lane, one_comm());
        }
        LegacyDemandWalkEvent::Unknown => legacy_bump_lane(&mut acc, envelope_key, DemandEntry {
            known_lower_bound: 0,
            unknown: true,
        }),
    });
    acc
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    scalar: DemandEntry,
    lanes: BTreeMap<SigKey, DemandEntry>,
    callback_count: u64,
    callback_state: u64,
}

fn update_callback_state(state: u64, encoded: &[u8]) -> u64 {
    let mut next = state ^ (encoded.len() as u64);
    for &byte in encoded {
        next ^= u64::from(byte);
        next = next.wrapping_mul(0x0000_0100_0000_01b3);
    }
    next.rotate_left(17) ^ 0x9e37_79b9_7f4a_7c15
}

fn run_arm(arm: Arm, fixture: &Par, first_channel: &[u8], second_channel: &[u8]) -> Outcome {
    let callback_count = Cell::new(0u64);
    let callback_state = Cell::new(CALLBACK_INITIAL_STATE);
    let callback = |channel: &Par| {
        let encoded = channel.encode_to_vec();
        callback_count.set(callback_count.get().saturating_add(1));
        callback_state.set(update_callback_state(callback_state.get(), &encoded));
        if encoded == first_channel {
            Some(FIRST_LANE)
        } else if encoded == second_channel {
            Some(SECOND_LANE)
        } else {
            None
        }
    };

    let envelope = Sig::Ground(vec![9, 8, 7, 6]);
    let (scalar, lanes) = match arm {
        Arm::Control => (
            legacy_demand_par(black_box(fixture)),
            legacy_demand_by_sig(black_box(fixture), ENVELOPE_KEY, &callback),
        ),
        Arm::Treatment => (
            demand(black_box(fixture), black_box(&envelope)),
            demand_by_sig(black_box(fixture), ENVELOPE_KEY, &callback),
        ),
    };
    Outcome {
        scalar,
        lanes,
        callback_count: callback_count.get(),
        callback_state: callback_state.get(),
    }
}

/// Stable Callgrind toggle boundary for deterministic instruction profiles.
/// Fixture construction and result serialization stay outside the wrapper;
/// launch-mode Massif and uProf still observe the whole process.
#[inline(never)]
fn profile_control(fixture: &Par, first_channel: &[u8], second_channel: &[u8]) -> Outcome {
    let outcome = run_arm(Arm::Control, fixture, first_channel, second_channel);
    black_box(&outcome);
    outcome
}

/// Tail-call-resistant Callgrind boundary for the current production treatment.
#[inline(never)]
fn profile_treatment(fixture: &Par, first_channel: &[u8], second_channel: &[u8]) -> Outcome {
    let outcome = run_arm(Arm::Treatment, fixture, first_channel, second_channel);
    black_box(&outcome);
    outcome
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    Control,
    Treatment,
}

impl Arm {
    const fn label(self) -> &'static str {
        match self {
            Self::Control => "control",
            Self::Treatment => "treatment",
        }
    }
}

fn witness(outcome: &Outcome) -> u64 {
    fn mix(mut state: u64, bytes: &[u8]) -> u64 {
        for &byte in bytes {
            state ^= u64::from(byte);
            state = state.wrapping_mul(0x0000_0100_0000_01b3);
        }
        state
    }

    let mut state = CALLBACK_INITIAL_STATE;
    state = mix(state, &outcome.scalar.known_lower_bound.to_le_bytes());
    state = mix(state, &[u8::from(outcome.scalar.unknown)]);
    for (lane, entry) in &outcome.lanes {
        state = mix(state, lane);
        state = mix(state, &entry.known_lower_bound.to_le_bytes());
        state = mix(state, &[u8::from(entry.unknown)]);
    }
    state = mix(state, &outcome.callback_count.to_le_bytes());
    mix(state, &outcome.callback_state.to_le_bytes())
}

fn validate_outcome(outcome: &Outcome) {
    assert_eq!(
        outcome.scalar,
        DemandEntry {
            known_lower_bound: 2,
            unknown: false,
        },
        "the fixture must retain two statically known COMMs"
    );
    assert_eq!(
        outcome.callback_count, 2,
        "both channels must reach the callback"
    );
    assert_ne!(
        outcome.callback_state, CALLBACK_INITIAL_STATE,
        "the stateful callback must be non-vacuous"
    );
    assert_eq!(
        outcome.lanes.len(),
        2,
        "both signer lanes must be represented"
    );
    assert_eq!(outcome.lanes.get(&FIRST_LANE), Some(&one_comm()));
    assert_eq!(outcome.lanes.get(&SECOND_LANE), Some(&one_comm()));
}

struct Measurement {
    arm: Arm,
    elapsed: Duration,
    outcome: Outcome,
    cpu_before: usize,
    cpu_after: usize,
}

fn measure(
    arm: Arm,
    fixture: &Par,
    first_channel: &[u8],
    second_channel: &[u8],
    expected_cpu: Option<usize>,
) -> Measurement {
    let cpu_before = current_cpu();
    if let Some(expected) = expected_cpu {
        assert_eq!(cpu_before, expected, "arm began on the wrong logical CPU");
    }
    let started = Instant::now();
    let outcome = black_box(run_arm(
        arm,
        black_box(fixture),
        first_channel,
        second_channel,
    ));
    let elapsed = started.elapsed();
    let cpu_after = current_cpu();
    if let Some(expected) = expected_cpu {
        assert_eq!(cpu_after, expected, "arm ended on the wrong logical CPU");
    }
    validate_outcome(&outcome);
    Measurement {
        arm,
        elapsed,
        outcome,
        cpu_before,
        cpu_after,
    }
}

fn write_metadata(
    output: &mut BufWriter<File>,
    config: &Config,
    protocol: Protocol,
    affinity: &[usize],
) -> std::io::Result<()> {
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_secs();
    writeln!(output, "# experiment_id={EXPERIMENT_ID}")?;
    writeln!(output, "# mode={}", config.mode.name())?;
    writeln!(output, "# seed=0x{SEED:x}")?;
    writeln!(output, "# depth={}", protocol.depth)?;
    writeln!(output, "# warmup_pairs={}", protocol.warmup_pairs)?;
    writeln!(output, "# retained_pairs={}", protocol.retained_pairs)?;
    writeln!(output, "# treatment_ref={}", config.treatment_ref)?;
    writeln!(output, "# frozen_control_git={FROZEN_CONTROL_GIT}")?;
    writeln!(output, "# frozen_source_sha256={FROZEN_SOURCE_SHA256}")?;
    writeln!(output, "# affinity_cpus={affinity:?}")?;
    writeln!(output, "# started_unix_seconds={started}")?;
    if let Some(profile) = config.profile {
        writeln!(output, "# profile_arm={}", profile.arm.label())?;
        writeln!(output, "# profile_iterations={}", profile.iterations)?;
    }
    writeln!(
        output,
        "phase\tpair\torder\tarm\telapsed_ns\tdepth\tseed\twitness\tcallback_count\tcallback_state\tcpu_before\tcpu_after\ttreatment_ref\tfrozen_control_git"
    )?;
    output.flush()
}

struct RowContext<'a> {
    phase: &'a str,
    pair: usize,
    order: &'a str,
    depth: usize,
    treatment_ref: &'a str,
}

fn write_measurement(
    output: &mut BufWriter<File>,
    context: RowContext<'_>,
    measurement: &Measurement,
) -> std::io::Result<()> {
    writeln!(
        output,
        "{}\t{}\t{}\t{}\t{}\t{}\t0x{:x}\t{:016x}\t{}\t{:016x}\t{}\t{}\t{}\t{}",
        context.phase,
        context.pair,
        context.order,
        measurement.arm.label(),
        measurement.elapsed.as_nanos(),
        context.depth,
        SEED,
        witness(&measurement.outcome),
        measurement.outcome.callback_count,
        measurement.outcome.callback_state,
        measurement.cpu_before,
        measurement.cpu_after,
        context.treatment_ref,
        FROZEN_CONTROL_GIT,
    )?;
    output.flush()
}

fn run_pair(
    output: &mut BufWriter<File>,
    context: RowContext<'_>,
    order: [Arm; 2],
    fixture: &Par,
    first_channel: &[u8],
    second_channel: &[u8],
    expected_cpu: Option<usize>,
) -> Duration {
    let pair_started = Instant::now();
    let mut control = None;
    let mut treatment = None;
    for arm in order {
        let measurement = measure(arm, fixture, first_channel, second_channel, expected_cpu);
        write_measurement(
            output,
            RowContext {
                phase: context.phase,
                pair: context.pair,
                order: context.order,
                depth: context.depth,
                treatment_ref: context.treatment_ref,
            },
            &measurement,
        )
        .expect("flush measurement row");
        match arm {
            Arm::Control => control = Some(measurement.outcome),
            Arm::Treatment => treatment = Some(measurement.outcome),
        }
    }
    assert_eq!(
        control.expect("control arm ran"),
        treatment.expect("treatment arm ran"),
        "frozen and treatment results diverged"
    );
    pair_started.elapsed()
}

fn run_profile(
    output: &mut BufWriter<File>,
    config: &Config,
    profile: ProfileConfig,
    fixture: &Par,
    first_channel: &[u8],
    second_channel: &[u8],
) {
    let expected_cpu = config.expected_cpu.expect("profile CPU");
    let cpu_before = current_cpu();
    assert_eq!(
        cpu_before, expected_cpu,
        "profile began on the wrong logical CPU"
    );
    let started = Instant::now();
    let mut last_outcome = None;
    for _ in 0..profile.iterations {
        let outcome = match profile.arm {
            Arm::Control => profile_control(fixture, first_channel, second_channel),
            Arm::Treatment => profile_treatment(fixture, first_channel, second_channel),
        };
        validate_outcome(&outcome);
        if let Some(previous) = last_outcome.as_ref() {
            assert_eq!(previous, &outcome, "profile repetitions diverged");
        }
        last_outcome = Some(black_box(outcome));
    }
    let elapsed = started.elapsed();
    let cpu_after = current_cpu();
    assert_eq!(
        cpu_after, expected_cpu,
        "profile ended on the wrong logical CPU"
    );
    let measurement = Measurement {
        arm: profile.arm,
        elapsed,
        outcome: last_outcome.expect("positive profile iteration count"),
        cpu_before,
        cpu_after,
    };
    write_measurement(
        output,
        RowContext {
            phase: "profile",
            pair: 0,
            order: "single",
            depth: profile.depth,
            treatment_ref: &config.treatment_ref,
        },
        &measurement,
    )
    .expect("flush profile row");
    eprintln!(
        "profile {} depth {} x{} completed in {:.6}s",
        profile.arm.label(),
        profile.depth,
        profile.iterations,
        elapsed.as_secs_f64()
    );
}

fn main() {
    let config = parse_config();
    let protocol = config.profile.map_or_else(
        || config.mode.protocol(),
        |profile| Protocol {
            depth: profile.depth,
            warmup_pairs: 0,
            retained_pairs: 0,
        },
    );
    let affinity = affinity_cpus();
    if matches!(config.mode, Mode::Locked | Mode::Profile) {
        assert_eq!(
            affinity,
            vec![config.expected_cpu.expect("exclusive CPU")],
            "locked and profile runs require an exact one-CPU affinity mask"
        );
    }
    if config.mode == Mode::Locked {
        eprintln!(
            "experiment 303 locked forecast: about 70–75 minutes; raw rows flush after every arm"
        );
    }

    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&config.output)
        .unwrap_or_else(|error| {
            panic!(
                "refusing to overwrite measurement output {}: {error}",
                config.output.display()
            )
        });
    let mut output = BufWriter::new(file);
    write_metadata(&mut output, &config, protocol, &affinity).expect("write metadata");

    eprintln!(
        "building depth-{} fixture outside the measurement window",
        protocol.depth
    );
    let (fixture, first_channel, second_channel) = deep_new_chain(protocol.depth);

    if let Some(profile) = config.profile {
        run_profile(
            &mut output,
            &config,
            profile,
            &fixture,
            &first_channel,
            &second_channel,
        );
        return;
    }

    let run_started = Instant::now();
    for pair in 0..protocol.warmup_pairs {
        let order = if pair % 2 == 0 {
            [Arm::Control, Arm::Treatment]
        } else {
            [Arm::Treatment, Arm::Control]
        };
        let order_label = if order[0] == Arm::Control { "AB" } else { "BA" };
        let elapsed = run_pair(
            &mut output,
            RowContext {
                phase: "warmup",
                pair,
                order: order_label,
                depth: protocol.depth,
                treatment_ref: &config.treatment_ref,
            },
            order,
            &fixture,
            &first_channel,
            &second_channel,
            config.expected_cpu,
        );
        eprintln!(
            "warmup pair {}/{} completed in {:.3}s",
            pair + 1,
            protocol.warmup_pairs,
            elapsed.as_secs_f64()
        );
    }

    for pair in 0..protocol.retained_pairs {
        let order = if pair % 2 == 0 {
            [Arm::Control, Arm::Treatment]
        } else {
            [Arm::Treatment, Arm::Control]
        };
        let order_label = if order[0] == Arm::Control { "AB" } else { "BA" };
        let elapsed = run_pair(
            &mut output,
            RowContext {
                phase: "retained",
                pair,
                order: order_label,
                depth: protocol.depth,
                treatment_ref: &config.treatment_ref,
            },
            order,
            &fixture,
            &first_channel,
            &second_channel,
            config.expected_cpu,
        );
        let completed = pair + 1;
        let seconds_per_pair =
            run_started.elapsed().as_secs_f64() / (protocol.warmup_pairs + completed) as f64;
        let remaining = seconds_per_pair * (protocol.retained_pairs - completed) as f64;
        eprintln!(
            "retained pair {completed}/{} ({order_label}) completed in {:.3}s; estimated {:.1} minutes remaining",
            protocol.retained_pairs,
            elapsed.as_secs_f64(),
            remaining / 60.0
        );
    }

    eprintln!(
        "{} completed in {:.1} minutes; raw measurements: {}",
        config.mode.name(),
        run_started.elapsed().as_secs_f64() / 60.0,
        config.output.display()
    );
}
