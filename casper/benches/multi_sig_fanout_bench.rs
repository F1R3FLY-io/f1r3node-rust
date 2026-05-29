//! Phase 4.17 — Criterion benchmarks for the multi-sig hot paths.
//!
//! Measures latency at the substrate boundary so future regressions
//! (e.g., quadratic blowups in signer-count, redundant Blake2b256
//! hashes per signer) surface in `cargo bench` output. NOT a CI gate
//! per the team's "local-only benchmarks" convention.
//!
//! Benchmarks:
//! - Cosigned::from_signed_data construction at N ∈ {1, 4, 16, 64}
//! - SignatureChannel::from_sig at random Sig depth ∈ {1, 3, 5}
//! - set_deploy_signatures fold latency at N ∈ {1, 4, 16, 64}
//! - Cosigned::from_signed_data_threshold at (n=64, k=32)
//!
//! Run:
//!   cargo bench -p casper --bench multi_sig_fanout_bench

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Cosigner, Signed, ToMessage};
use models::rust::casper::protocol::casper_message::DeployData;
use prost::bytes::Bytes;
use prost::Message;
use rholang::rust::interpreter::accounting::{
    costs::Cost, RuntimeBudget, Sig, SignatureChannel,
};

fn baseline_deploy_data(phlo_limit: i64) -> DeployData {
    DeployData {
        term: "Nil".to_string(),
        time_stamp: 1700000000000,
        phlo_price: 1,
        phlo_limit,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    }
}

fn build_n_signers(data: &DeployData, n: usize) -> Vec<Cosigner> {
    let secp = Secp256k1;
    let share = data.phlo_limit / (n as i64);
    let leftover = data.phlo_limit - share * (n as i64);
    let serialized = data.to_message().encode_to_vec();
    let hash = Signed::<DeployData>::signature_hash(&Secp256k1::name(), serialized);
    (0..n)
        .map(|i| {
            let (sk, pk) = secp.new_key_pair();
            let sig = Bytes::from(secp.sign(&hash, &sk.bytes));
            Cosigner {
                pk,
                sig,
                sig_algorithm: Box::new(Secp256k1),
                phlo_share: if i == 0 { share + leftover } else { share },
            }
        })
        .collect()
}

fn bench_cosigned_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("Cosigned::from_signed_data");
    for n in [1usize, 4, 16, 64].iter().copied() {
        let data = baseline_deploy_data(1024 * (n as i64));
        let signers = build_n_signers(&data, n);
        group.bench_with_input(
            BenchmarkId::from_parameter(n),
            &(data, signers),
            |b, (data, signers)| {
                b.iter(|| {
                    let cosigned = Cosigned::from_signed_data(
                        black_box(data.clone()),
                        black_box(signers.clone()),
                        black_box(1024 * (signers.len() as i64)),
                    )
                    .expect("envelope construction");
                    black_box(cosigned);
                });
            },
        );
    }
    group.finish();
}

fn bench_signature_channel_from_sig(c: &mut Criterion) {
    fn make_sig(depth: usize) -> Sig {
        if depth == 0 {
            Sig::Ground(vec![0xCC])
        } else {
            Sig::And(
                Box::new(make_sig(depth - 1)),
                Box::new(make_sig(depth - 1)),
            )
        }
    }
    let mut group = c.benchmark_group("SignatureChannel::from_sig");
    for depth in [1usize, 3, 5].iter().copied() {
        let sig = make_sig(depth);
        group.bench_with_input(
            BenchmarkId::new("And-tree", depth),
            &sig,
            |b, sig| {
                b.iter(|| {
                    let channel = SignatureChannel::from_sig(black_box(sig));
                    black_box(channel);
                });
            },
        );
    }
    group.finish();
}

fn bench_set_deploy_signatures(c: &mut Criterion) {
    let mut group = c.benchmark_group("RuntimeBudget::set_deploy_signatures");
    for n in [1usize, 4, 16, 64].iter().copied() {
        let sigs: Vec<Vec<u8>> = (0..n).map(|i| vec![0xCC + (i as u8); 32]).collect();
        group.bench_with_input(
            BenchmarkId::from_parameter(n),
            &sigs,
            |b, sigs| {
                let refs: Vec<&[u8]> = sigs.iter().map(Vec::as_slice).collect();
                b.iter(|| {
                    let budget = RuntimeBudget::new(Cost::create(1024, "bench"));
                    budget.set_deploy_signatures(black_box(&refs));
                    black_box(budget.deploy_id());
                });
            },
        );
    }
    group.finish();
}

fn bench_cosigned_threshold_64_choose_32(c: &mut Criterion) {
    let data = baseline_deploy_data(32 * 1024);
    let signers = build_n_signers(&data, 32);
    // Pad with 32 placeholder signers (empty sig, zero share) for n=64.
    let secp = Secp256k1;
    let mut all_signers = signers;
    for _ in 0..32 {
        let (_, pk) = secp.new_key_pair();
        all_signers.push(Cosigner {
            pk,
            sig: Bytes::new(),
            sig_algorithm: Box::new(Secp256k1),
            phlo_share: 0,
        });
    }
    c.bench_function("Cosigned::from_signed_data_threshold/64-of-32", |b| {
        b.iter(|| {
            let cosigned = Cosigned::from_signed_data_threshold(
                black_box(data.clone()),
                black_box(all_signers.clone()),
                black_box(32 * 1024),
                black_box(32),
            )
            .expect("threshold envelope construction");
            black_box(cosigned);
        });
    });
}

criterion_group!(
    benches,
    bench_cosigned_construction,
    bench_signature_channel_from_sig,
    bench_set_deploy_signatures,
    bench_cosigned_threshold_64_choose_32
);
criterion_main!(benches);
