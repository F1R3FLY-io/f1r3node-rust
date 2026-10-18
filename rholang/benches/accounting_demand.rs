use std::collections::BTreeMap;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{Bundle, EVar, Expr, If, Match, New, Par, Receive, ReceiveBind, Send, Var};
use models::rust::par_construction::ParConstruction;
use rholang::rust::interpreter::accounting::delta_sigma::{
    demand, demand_by_sig, demand_by_sig_encoded, DemandEntry,
};
use rholang::rust::interpreter::accounting::Sig;
use rspace_plus_plus::rspace::bag::Multiplicity;

/// Criterion filters execution after the benchmark-registration closure has
/// run.  Keep focused profiler invocations from eagerly constructing every
/// unrelated deep fixture before the selected benchmark starts.
fn selected_benchmark(name: &str) -> bool {
    std::env::args()
        .nth(1)
        .is_none_or(|filter| filter.starts_with('-') || name.contains(&filter))
}

fn deep_new_chain(depth: usize) -> Par {
    let mut construction = ParConstruction::new();
    let empty = construction.empty_par().expect("planned empty Par");
    let send_shell = Send {
        chan: Some(Par::default()),
        data: Vec::new(),
        persistent: false,
        locally_free: Vec::new(),
        connective_used: false,
        cost_region: None,
    };
    let send = construction
        .send_component(&send_shell, &[empty])
        .expect("planned terminal Send");
    let mut planned = construction
        .singleton_component(send, Vec::new(), false)
        .expect("planned terminal Par");
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
    construction.finish(planned).expect("finish planned Par")
}

enum ControlTask {
    Visit(Par),
    ScanUnknown(Par),
}

/// Exact pre-byte-PDA control retained only in this benchmark target.
fn suffix_materializing_control(root: &Par) -> DemandEntry {
    let mut result = DemandEntry::ZERO;
    let mut tasks = vec![ControlTask::Visit(root.clone())];
    while let Some(task) = tasks.pop() {
        match task {
            ControlTask::ScanUnknown(par) => {
                for expr in par.decode_occurrences::<Expr>() {
                    if let Some(ExprInstance::EVarBody(evar)) = &expr.expr_instance {
                        if matches!(
                            evar.v.as_ref().and_then(|var| var.var_instance.as_ref()),
                            Some(VarInstance::BoundVar(_)) | Some(VarInstance::FreeVar(_))
                        ) {
                            result.unknown = true;
                        }
                    }
                }
            }
            ControlTask::Visit(mut par) => {
                tasks.push(ControlTask::ScanUnknown(par.clone()));
                for bundle in par.take_occurrences::<Bundle>().into_iter().rev() {
                    if let Some(body) = bundle.body {
                        tasks.push(ControlTask::Visit(body));
                    }
                }
                for conditional in par.take_occurrences::<If>().into_iter().rev() {
                    if let Some(if_false) = conditional.if_false {
                        tasks.push(ControlTask::Visit(if_false));
                    }
                    if let Some(if_true) = conditional.if_true {
                        tasks.push(ControlTask::Visit(if_true));
                    }
                }
                for mat in par.take_occurrences::<Match>().into_iter().rev() {
                    for case in mat.cases.into_iter().rev() {
                        if let Some(source) = case.source {
                            tasks.push(ControlTask::Visit(source));
                        }
                    }
                }
                for new in par.take_occurrences::<New>().into_iter().rev() {
                    if let Some(body) = new.p {
                        tasks.push(ControlTask::Visit(body));
                    }
                }
                for receive in par.take_occurrences::<Receive>().into_iter().rev() {
                    if let Some(body) = receive.body {
                        tasks.push(ControlTask::Visit(body));
                    }
                    result.known_lower_bound = result.known_lower_bound.saturating_add(1);
                }
                result.known_lower_bound = result.known_lower_bound.saturating_add(
                    i64::try_from(par.family_cardinality::<Send>()).unwrap_or(i64::MAX),
                );
            }
        }
    }
    result
}

fn byte_channel(id: usize, payload_len: usize) -> Par {
    let mut bytes = vec![0x5a; payload_len.max(8)];
    bytes[..8].copy_from_slice(&(id as u64).to_le_bytes());
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GByteArray(bytes)),
    }])
}

fn wide_send_fixture(width: usize, channel_bytes: usize) -> Par {
    Par::default().with_sends(
        (0..width)
            .map(|id| Send {
                chan: Some(byte_channel(id, channel_bytes)),
                data: Vec::new(),
                persistent: id % 2 == 0,
                locally_free: Vec::new(),
                connective_used: false,
                cost_region: None,
            })
            .collect(),
    )
}

fn compressed_trace_fixture(multiplicity: u64) -> Par {
    let body = wide_send_fixture(2, 32);
    let receive = Receive {
        binds: vec![ReceiveBind {
            patterns: Vec::new(),
            source: Some(byte_channel(99, 32)),
            remainder: None,
            free_count: 0,
            cost_region: None,
        }],
        body: Some(body),
        persistent: false,
        peek: false,
        bind_count: 0,
        locally_free: Vec::new(),
        connective_used: false,
        condition: None,
        cost_region: None,
    };
    let mut par = Par::default();
    par.insert_component_many(
        &receive,
        Multiplicity::new(multiplicity).expect("nonzero benchmark multiplicity"),
        Vec::new(),
        false,
    )
    .expect("compressed benchmark endpoint");
    par
}

fn unknown_fixture(width: usize) -> Par {
    Par::default().with_exprs(
        (0..width)
            .map(|index| Expr {
                expr_instance: Some(ExprInstance::EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(VarInstance::BoundVar(index as i32)),
                    }),
                })),
            })
            .collect(),
    )
}

fn accounting_demand_benchmark(criterion: &mut Criterion) {
    let envelope = Sig::Ground(vec![9, 8, 7, 6]);

    let mut treatment = criterion.benchmark_group("accounting_demand/byte_pda");
    treatment.sample_size(30);
    treatment.warm_up_time(Duration::from_secs(1));
    treatment.measurement_time(Duration::from_secs(4));
    for depth in [128usize, 512, 2_048, 8_192, 20_000] {
        if !selected_benchmark(&format!("accounting_demand/byte_pda/{depth}")) {
            continue;
        }
        let par = deep_new_chain(depth);
        assert_eq!(demand(&par, &envelope).known_lower_bound, 1);
        treatment.throughput(Throughput::Elements(depth as u64));
        treatment.bench_with_input(BenchmarkId::from_parameter(depth), &par, |bencher, par| {
            bencher.iter(|| black_box(demand(black_box(par), black_box(&envelope))));
        });
    }
    treatment.finish();

    let mut control = criterion.benchmark_group("accounting_demand/suffix_control");
    control.sample_size(30);
    control.warm_up_time(Duration::from_secs(1));
    control.measurement_time(Duration::from_secs(4));
    for depth in [64usize, 128, 256, 512] {
        if !selected_benchmark(&format!("accounting_demand/suffix_control/{depth}")) {
            continue;
        }
        let par = deep_new_chain(depth);
        assert_eq!(suffix_materializing_control(&par), demand(&par, &envelope));
        control.throughput(Throughput::Elements(depth as u64));
        control.bench_with_input(BenchmarkId::from_parameter(depth), &par, |bencher, par| {
            bencher.iter(|| black_box(suffix_materializing_control(black_box(par))));
        });
    }
    control.finish();

    let mut wide = criterion.benchmark_group("accounting_demand/wide_scalar");
    wide.sample_size(30);
    wide.warm_up_time(Duration::from_secs(1));
    wide.measurement_time(Duration::from_secs(4));
    for width in [16usize, 64, 256] {
        if !selected_benchmark(&format!("accounting_demand/wide_scalar/{width}")) {
            continue;
        }
        let par = wide_send_fixture(width, 4_096);
        assert_eq!(demand(&par, &envelope).known_lower_bound, width as i64);
        wide.throughput(Throughput::Elements(width as u64));
        wide.bench_with_input(BenchmarkId::from_parameter(width), &par, |bencher, par| {
            bencher.iter(|| black_box(demand(black_box(par), black_box(&envelope))));
        });
    }
    wide.finish();

    let envelope_key = [0xee; 32];
    let mut lane_native = criterion.benchmark_group("accounting_demand/lane_byte_native");
    lane_native.sample_size(30);
    lane_native.warm_up_time(Duration::from_secs(1));
    lane_native.measurement_time(Duration::from_secs(4));
    for width in [16usize, 64] {
        if !selected_benchmark(&format!("accounting_demand/lane_byte_native/{width}")) {
            continue;
        }
        let par = wide_send_fixture(width, 4_096);
        lane_native.throughput(Throughput::Elements(width as u64));
        lane_native.bench_with_input(BenchmarkId::from_parameter(width), &par, |bencher, par| {
            bencher.iter(|| {
                black_box(demand_by_sig_encoded(black_box(par), envelope_key, &|_| {
                    None
                }))
            });
        });
    }
    lane_native.finish();

    let mut lane_compat = criterion.benchmark_group("accounting_demand/lane_par_adapter");
    lane_compat.sample_size(30);
    lane_compat.warm_up_time(Duration::from_secs(1));
    lane_compat.measurement_time(Duration::from_secs(4));
    for width in [16usize, 64] {
        if !selected_benchmark(&format!("accounting_demand/lane_par_adapter/{width}")) {
            continue;
        }
        let par = wide_send_fixture(width, 4_096);
        assert_eq!(
            demand_by_sig(&par, envelope_key, &|_| None),
            demand_by_sig_encoded(&par, envelope_key, &|_| None)
        );
        lane_compat.throughput(Throughput::Elements(width as u64));
        lane_compat.bench_with_input(BenchmarkId::from_parameter(width), &par, |bencher, par| {
            bencher.iter(|| black_box(demand_by_sig(black_box(par), envelope_key, &|_| None)));
        });
    }
    lane_compat.finish();

    let mut multiplicity = criterion.benchmark_group("accounting_demand/compressed_scalar");
    multiplicity.sample_size(30);
    multiplicity.warm_up_time(Duration::from_secs(1));
    multiplicity.measurement_time(Duration::from_secs(4));
    for count in [1u64, 1_024, u64::from(u32::MAX)] {
        if !selected_benchmark(&format!("accounting_demand/compressed_scalar/{count}")) {
            continue;
        }
        let par = compressed_trace_fixture(count);
        assert_eq!(
            demand(&par, &envelope).known_lower_bound,
            i64::try_from(3u128 * u128::from(count)).unwrap_or(i64::MAX)
        );
        multiplicity.bench_with_input(BenchmarkId::from_parameter(count), &par, |bencher, par| {
            bencher.iter(|| black_box(demand(black_box(par), black_box(&envelope))));
        });
    }
    multiplicity.finish();

    let mut replay = criterion.benchmark_group("accounting_demand/compressed_lane_replay");
    replay.sample_size(30);
    replay.warm_up_time(Duration::from_secs(1));
    replay.measurement_time(Duration::from_secs(4));
    for count in [1u64, 64, 1_024] {
        if !selected_benchmark(&format!("accounting_demand/compressed_lane_replay/{count}")) {
            continue;
        }
        let par = compressed_trace_fixture(count);
        replay.throughput(Throughput::Elements(3 * count));
        replay.bench_with_input(BenchmarkId::from_parameter(count), &par, |bencher, par| {
            bencher.iter(|| {
                black_box(demand_by_sig_encoded(black_box(par), envelope_key, &|_| {
                    None
                }))
            });
        });
    }
    replay.finish();

    let mut unknown = criterion.benchmark_group("accounting_demand/unknown_scan");
    unknown.sample_size(30);
    unknown.warm_up_time(Duration::from_secs(1));
    unknown.measurement_time(Duration::from_secs(4));
    for width in [16usize, 64, 256] {
        if !selected_benchmark(&format!("accounting_demand/unknown_scan/{width}")) {
            continue;
        }
        let par = unknown_fixture(width);
        assert!(demand(&par, &envelope).unknown);
        unknown.throughput(Throughput::Elements(width as u64));
        unknown.bench_with_input(BenchmarkId::from_parameter(width), &par, |bencher, par| {
            bencher.iter(|| black_box(demand(black_box(par), black_box(&envelope))));
        });
    }
    unknown.finish();
}

criterion_group!(benches, accounting_demand_benchmark);
criterion_main!(benches);
