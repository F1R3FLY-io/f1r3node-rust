use std::cell::Cell;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::replay_rspace::native_epoch::NativeCandidateIdentity;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

use super::super::operation_sources::channel_bytes;
use super::*;

#[test]
fn native_operation_source_preserves_channel_persistence_and_counter_presence() {
    let host = config(100, [0; 4]).host_work;
    let source = Produce::create(&1u8, &7u8, false);
    let captured =
        NativeOperationSource::capture(RSpaceOperationSource::Produce(&source), &host).unwrap();
    for change in 0..2 {
        let mut different = source.clone();
        if change == 0 {
            different.channel_hash = Produce::create(&2u8, &7u8, false).channel_hash;
        } else {
            different.persistent = true;
        }
        assert_eq!(source, different);
        assert!(!captured
            .metered_matches(
                RSpaceOperationSource::Produce(&different),
                &|_, _, _| Ok(())
            )
            .unwrap());
        assert_ne!(
            captured,
            NativeOperationSource::capture(RSpaceOperationSource::Produce(&different), &host)
                .unwrap()
        );
    }
    let mut comm = COMM {
        consume: Consume::create(&vec![1u8], &vec![0u8], &7u8, false),
        produces: vec![source.clone()],
        peeks: Default::default(),
        times_repeated: Default::default(),
    };
    let absent = NativeCommSource::capture(&comm, &host).unwrap();
    comm.times_repeated.insert(source.clone(), 0);
    let zero = NativeCommSource::capture(&comm, &host).unwrap();
    assert_ne!(absent, zero);
    comm.times_repeated.clear();
    let mut changed = source;
    changed.persistent = true;
    comm.times_repeated.insert(changed, 0);
    assert_ne!(zero, NativeCommSource::capture(&comm, &host).unwrap());
}

#[test]
fn native_operation_channel_encoding_rejects_host_exhaustion() {
    let host = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(0)));
    assert!(matches!(
        channel_bytes(&Par::default(), &host),
        Err(InterpreterError::HostWorkRejected)
    ));
    assert!(host.is_rejected());
}

#[test]
fn native_candidate_identity_uses_full_sources_without_imported_telemetry() {
    let host = config(100, [0; 4]).host_work;
    let producer = Produce::create(&1u8, &7u8, false);
    let source = COMM {
        consume: Consume::create(&vec![1u8], &vec![0u8], &7u8, false),
        produces: vec![producer.clone()],
        peeks: Default::default(),
        times_repeated: [(producer.clone(), 3)].into_iter().collect(),
    };
    let identity = NativeCommSource::capture(&source, &host).unwrap();
    assert!(identity.matches_consume(&source.consume));
    assert!(identity.matches_produce(&producer));
    assert_eq!(identity.repetition(&producer), Some(3));
    assert!(identity.matches_comm(&source));
    let mut telemetry = producer.clone();
    telemetry.output_value = vec![vec![1, 2, 3]];
    telemetry.is_deterministic = false;
    telemetry.failed = true;
    assert!(identity.matches_produce(&telemetry));
    assert_eq!(identity.repetition(&telemetry), Some(3));
    for change in 0..3 {
        let mut different = producer.clone();
        match change {
            0 => different.persistent = true,
            1 => different.channel_hash = Produce::create(&2u8, &7u8, false).channel_hash,
            _ => different.hash = Produce::create(&1u8, &8u8, false).hash,
        }
        assert!(!identity.matches_produce(&different));
        assert_eq!(identity.repetition(&different), None);
    }
}

#[test]
fn native_candidate_identity_pays_each_comparison_before_evaluation() {
    let host = config(100, [0; 4]).host_work;
    let producer = Produce::create(&1u8, &7u8, false);
    let source = COMM {
        consume: Consume::create(&vec![1u8], &vec![0u8], &7u8, false),
        produces: vec![producer.clone()],
        peeks: [0].into_iter().collect(),
        times_repeated: [(producer.clone(), 3)].into_iter().collect(),
    };
    let identity = NativeCommSource::capture(&source, &host).unwrap();
    let usage = Cell::new([0usize; 3]);
    let unlimited = |operations: usize, scanned: usize, backing: usize| {
        let mut next = usage.get();
        for (used, added) in next.iter_mut().zip([operations, scanned, backing]) {
            *used += added;
        }
        usage.set(next);
        Ok(())
    };
    assert!(identity
        .metered_matches_consume(&source.consume, &unlimited)
        .unwrap());
    assert!(identity
        .metered_matches_produce(&producer, &unlimited)
        .unwrap());
    assert_eq!(
        identity.metered_repetition(&producer, &unlimited).unwrap(),
        Some(3)
    );
    usage.set([0; 3]);
    assert!(identity.metered_matches_comm(&source, &unlimited).unwrap());
    let required = usage.get();
    assert!(required[0] > 1);
    assert!(required[1] > 0);
    for limit in 0..required[0] {
        let used = Cell::new(0usize);
        let meter = |count: usize, _: usize, _: usize| {
            let next = used.get() + count;
            if next > limit {
                return Err(RSpaceError::HostWorkRejected);
            }
            used.set(next);
            Ok(())
        };
        assert!(matches!(
            identity.metered_matches_comm(&source, &meter),
            Err(RSpaceError::HostWorkRejected)
        ));
    }
    for dimension in 0..2 {
        let mut limits = required;
        limits[dimension] -= 1;
        let used = Cell::new([0usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let mut next = used.get();
            for (value, added) in next.iter_mut().zip([operations, scanned, backing]) {
                *value += added;
            }
            if next.iter().zip(limits).any(|(value, limit)| *value > limit) {
                return Err(RSpaceError::HostWorkRejected);
            }
            used.set(next);
            Ok(())
        };
        assert!(matches!(
            identity.metered_matches_comm(&source, &meter),
            Err(RSpaceError::HostWorkRejected)
        ));
    }
    let mut changed = source.clone();
    changed.consume.channel_hashes[0] =
        Consume::create(&vec![2u8], &vec![0u8], &7u8, false).channel_hashes[0].clone();
    assert_eq!(
        identity.metered_matches_comm(&changed, &unlimited).unwrap(),
        identity.matches_comm(&changed)
    );
    assert!(!identity.metered_matches_comm(&changed, &unlimited).unwrap());
    let mut changed = source.clone();
    changed.peeks.insert(1);
    assert!(!identity.metered_matches_comm(&changed, &unlimited).unwrap());
    let rejected = |_: usize, _: usize, _: usize| Err(RSpaceError::HostWorkRejected);
    assert!(matches!(
        identity.metered_matches_produce(&producer, &rejected),
        Err(RSpaceError::HostWorkRejected)
    ));
    assert!(matches!(
        identity.metered_repetition(&producer, &rejected),
        Err(RSpaceError::HostWorkRejected)
    ));
}

proptest! {
    #[test]
    fn native_comm_authentication_preserves_ordered_multi_party_sources(
        values in prop::collection::vec(any::<u64>(), 2..8),
    ) {
        let host = config(100, [0; 4]).host_work;
        let channels: Vec<_> = (0..values.len()).map(|i| i as u64).collect();
        let produces: Vec<_> = values.iter().enumerate().map(|(i, value)|
            Produce::create(&(i as u64), &(i as u64, *value), i % 2 == 0)).collect();
        let source = COMM {
            consume: Consume::create(&channels, &vec![0u8; values.len()], &7u8, false),
            produces: produces.clone(),
            peeks: [0].into_iter().collect(),
            times_repeated: produces.into_iter().map(|source| (source, 0)).collect(),
        };
        let expected = NativeCommSource::capture(&source, &host).unwrap();
        let meter = |_, _, _| Ok(());
        prop_assert!(expected.metered_matches(&source, &meter).unwrap());
        prop_assert!(expected.matches(&source));
        let mut reordered = source.clone();
        reordered.produces.reverse();
        prop_assert!(!expected.matches(&reordered));
        let mut reordered = source.clone();
        reordered.consume.channel_hashes.reverse();
        prop_assert!(!expected.matches(&reordered));
        let mut different_counter = source;
        let (mut producer, count) = different_counter.times_repeated.pop_first().unwrap();
        producer.persistent = !producer.persistent;
        different_counter.times_repeated.insert(producer, count);
        prop_assert!(!expected.matches(&different_counter));
    }

    #[test]
    fn native_operation_channel_encoding_matches_rspace_bincode(
        values in prop::collection::vec(any::<i64>(), 0..128),
    ) {
        let channel = Par::default().with_exprs(values.into_iter().map(|value| Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }).collect());
        let host = config(100, [0; 4]).host_work;
        let encoded = channel_bytes(&channel, &host).unwrap();
        prop_assert_eq!(encoded.as_ref(), bincode::serialize(&channel).unwrap());
    }
}
