//! Criterion microbenchmark for the W1 Phase 3 metering hot path.
//!
//! Confirms the per-COMM located-stack attribution (`note_channel_lane`) adds
//! NEGLIGIBLE overhead on the single-signer fast path: it short-circuits on the
//! `any_signed_regions` flag (one atomic load) BEFORE any channel encode / signer
//! match, so a single-signer deploy's per-COMM cost is unchanged `reserve_comm`
//! plus ~one atomic load. The channel-encode + linear signer-match cost is paid
//! ONLY on the multi-signer path. The COMM charge (`reserve_comm`) itself is
//! untouched by Phase 3, so it is not benchmarked for regression here.
//!
//! NOT a CI gate (the team's local-only-benchmarks convention).
//!
//! Run:  cargo bench -p rholang --bench metering_bench

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use models::rhoapi::Par;
use rholang::rust::interpreter::accounting::costs::Cost;
use rholang::rust::interpreter::accounting::{envelope_sig_compound, RuntimeBudget};
use rholang::rust::interpreter::metering::MeteredMachine;

fn bench_metering(c: &mut Criterion) {
    // Single-signer fast path: `note_channel_lane` sees `any_signed_regions ==
    // false` and returns immediately (one atomic load, no encode/match/tally).
    // This is the per-COMM cost Phase 3 adds to every legacy deploy.
    {
        let budget = RuntimeBudget::new(Cost::create(i64::MAX, "metering bench"));
        budget.set_deploy_signature(b"single-signer-bench");
        let machine = MeteredMachine::new(budget);
        let data_channel = Par::default();
        c.bench_function("note_channel_lane_single_sig_fast_path", |b| {
            b.iter(|| machine.note_channel_lane(black_box(&data_channel)))
        });
    }

    // Multi-signer path: `note_channel_lane` encodes the channel and matches it
    // against the installed signer channels (here a 2-cosigner envelope), tallying
    // a hit to the leaf lane. This is the cost ONLY a multi-signer deploy pays.
    {
        let budget = RuntimeBudget::new(Cost::create(i64::MAX, "metering bench"));
        budget.set_deploy_signatures(&[b"cosigner-a", b"cosigner-b"]);
        let machine = MeteredMachine::new(budget);
        let leaf_channel = envelope_sig_compound(&[b"cosigner-a", b"cosigner-b"])
            .signer_channels()[0]
            .0
            .clone();
        c.bench_function("note_channel_lane_multi_sig_match", |b| {
            b.iter(|| machine.note_channel_lane(black_box(&leaf_channel)))
        });
    }
}

criterion_group!(benches, bench_metering);
criterion_main!(benches);
