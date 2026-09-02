//! Geometric causal benchmark for channel-local RSpace publication.
//!
//! `prefix-local` is the production transaction. `whole-store-clone` freezes
//! the removed transaction shape by explicitly cloning the store, mutating the
//! clone, and publishing it once per channel. Both arms call the same current
//! semantic insertion routine and must produce identical counts; only root
//! ownership mechanics differ.

use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use rspace_plus_plus::rspace::data_index::RSpaceDataStore;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::Datum;
use rspace_plus_plus::rspace::trace::event::Produce;

const TREATMENT_SIZES: [usize; 5] = [1_000, 2_000, 4_000, 8_000, 16_000];
const CONTROL_SIZES: [usize; 5] = [64, 128, 256, 512, 1_024];
const LOADED_REPLACEMENT_SIZES: [usize; 5] = [1_000, 2_000, 4_000, 8_000, 16_000];
const RETAINED_TRIALS: usize = 5;

#[derive(Clone, Copy)]
enum Arm {
    PrefixLocal,
    WholeStoreClone,
}

impl Arm {
    const fn label(self) -> &'static str {
        match self {
            Self::PrefixLocal => "prefix-local",
            Self::WholeStoreClone => "whole-store-clone",
        }
    }
}

fn datum(index: usize) -> Datum<String> {
    let discriminator = (index as u64).to_le_bytes();
    Datum {
        a: Arc::new(format!("payload-{index:08}")),
        persist: false,
        source: Produce::new(
            Blake2b256Hash::new(&discriminator),
            Blake2b256Hash::new(&[&discriminator[..], b"seq"].concat()),
            false,
        ),
    }
}

fn measure(arm: Arm, channels: usize) -> (u128, usize, u64, usize) {
    let started = Instant::now();
    let mut store = RSpaceDataStore::<String, String>::new();
    for index in 0..channels {
        let channel = format!("channel-{index:08}");
        let value = datum(index);
        match arm {
            Arm::PrefixLocal => store.set_channel(channel, [value]).unwrap(),
            Arm::WholeStoreClone => {
                let mut replacement = store.clone();
                replacement.set_channel(channel, [value]).unwrap();
                store = replacement;
            }
        }
    }
    let elapsed_ns = started.elapsed().as_nanos();
    let distinct = black_box(store.distinct_len());
    let cardinality = black_box(store.cardinality());
    let loaded = black_box(store.loaded_channel_count());
    assert_eq!(distinct, channels);
    assert_eq!(cardinality, channels as u64);
    assert_eq!(loaded, channels);
    drop(store);
    (elapsed_ns, distinct, cardinality, loaded)
}

fn measure_loaded_replacement(
    resident_channels: usize,
    replacement_ordinal: usize,
) -> (u128, usize, u64, usize) {
    assert_ne!(resident_channels, 0);
    let mut store = RSpaceDataStore::<String, String>::new();
    for index in 0..resident_channels {
        store
            .set_channel(format!("channel-{index:08}"), [datum(index)])
            .unwrap();
    }

    let replacement = datum(
        resident_channels
            .checked_add(replacement_ordinal)
            .expect("benchmark ordinals fit usize"),
    );
    let started = Instant::now();
    store
        .set_channel("channel-00000000".to_owned(), [replacement])
        .unwrap();
    let elapsed_ns = started.elapsed().as_nanos();

    let distinct = black_box(store.distinct_len());
    let cardinality = black_box(store.cardinality());
    let loaded = black_box(store.loaded_channel_count());
    assert_eq!(distinct, resident_channels);
    assert_eq!(cardinality, resident_channels as u64);
    assert_eq!(loaded, resident_channels);
    drop(store);
    (elapsed_ns, distinct, cardinality, loaded)
}

fn main() {
    println!("META\trspace-channel-prefix-publication-v1\ttrials={RETAINED_TRIALS}");
    println!("OBS_HEADER\tarm\tchannels\ttrial\telapsed_ns\tdistinct\tcardinality\tloaded");
    for (arm, sizes) in [
        (Arm::PrefixLocal, TREATMENT_SIZES.as_slice()),
        (Arm::WholeStoreClone, CONTROL_SIZES.as_slice()),
    ] {
        for channels in sizes {
            let _ = measure(arm, (*channels).min(64));
            for trial in 0..RETAINED_TRIALS {
                let (elapsed_ns, distinct, cardinality, loaded) = measure(arm, *channels);
                println!(
                    "OBS\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    arm.label(),
                    channels,
                    trial,
                    elapsed_ns,
                    distinct,
                    cardinality,
                    loaded
                );
            }
        }
    }

    println!("META\trspace-loaded-channel-replacement-v1\ttrials={RETAINED_TRIALS}");
    println!(
        "LOADED_OBS_HEADER\tresident_channels\ttrial\telapsed_ns\tdistinct\tcardinality\tloaded"
    );
    for resident_channels in LOADED_REPLACEMENT_SIZES {
        let _ = measure_loaded_replacement(resident_channels.min(64), 0);
        for trial in 0..RETAINED_TRIALS {
            let (elapsed_ns, distinct, cardinality, loaded) =
                measure_loaded_replacement(resident_channels, trial);
            println!(
                "LOADED_OBS\t{}\t{}\t{}\t{}\t{}\t{}",
                resident_channels, trial, elapsed_ns, distinct, cardinality, loaded
            );
        }
    }
}
