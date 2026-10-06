use std::cell::Cell;

use proptest::prelude::*;

use super::*;

#[derive(Default)]
struct Meter {
    used: Cell<[usize; 3]>,
    limit: Option<[usize; 3]>,
}

impl SourceMeter for Meter {
    fn reserve(&self, operations: usize, scanned: usize, backing: usize) -> Result<()> {
        let old = self.used.get();
        let add = [operations, scanned, backing];
        let mut next = old;
        for i in 0..3 {
            next[i] = old[i]
                .checked_add(add[i])
                .ok_or(RSpaceError::HostWorkRejected)?;
            if self.limit.is_some_and(|limit| next[i] > limit[i]) {
                return Err(RSpaceError::HostWorkRejected);
            }
        }
        self.used.set(next);
        Ok(())
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn constructors_preserve_legacy_source_identity_and_metadata(
        channels in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..128), 0..16),
        patterns in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..128), 0..16),
        data in prop::collection::vec(any::<u8>(), 0..256),
        persistent in any::<bool>(),
    ) {
        let meter = Meter::default();
        let actual = produce(&channels, &data, persistent, &meter).unwrap();
        let expected = Produce::create(&channels, &data, persistent);
        prop_assert_eq!(bincode::serialize(&actual).unwrap(), bincode::serialize(&expected).unwrap());
        let actual = consume(&channels, &patterns, &data, persistent, &meter).unwrap();
        prop_assert_eq!(actual, Consume::create(&channels, &patterns, &data, persistent));
    }

    #[test]
    fn all_three_budgets_reject_incomplete_construction_without_changing_inputs(
        channels in prop::collection::vec(any::<u64>(), 1..16),
        patterns in prop::collection::vec(any::<u64>(), 1..16),
        data in prop::collection::vec(any::<u8>(), 1..256),
        fraction in 0_usize..100,
    ) {
        let baseline = Meter::default();
        consume(&channels, &patterns, &data, true, &baseline).unwrap();
        let required = baseline.used.get();
        let before = bincode::serialize(&(&channels, &patterns, &data)).unwrap();
        for dimension in 0..3 {
            let mut limits = [usize::MAX; 3];
            limits[dimension] = required[dimension] * fraction / 100;
            let meter = Meter { used: Cell::new([0; 3]), limit: Some(limits) };
            prop_assert!(matches!(consume(&channels, &patterns, &data, true, &meter), Err(RSpaceError::HostWorkRejected)));
            prop_assert!(meter.used.get()[dimension] <= limits[dimension]);
        }
        prop_assert_eq!(before, bincode::serialize(&(&channels, &patterns, &data)).unwrap());
    }

    #[test]
    fn streaming_encoding_preserves_bincode_framing(
        values in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..128), 0..32),
    ) {
        let meter = Meter::default();
        let actual = encode(&values, &meter).unwrap();
        prop_assert_eq!(&actual, &bincode::serialize(&values).unwrap());
        prop_assert!(meter.used.get()[2] >= actual.len());
    }
}

#[test]
fn rejected_write_does_not_update_bytes_or_hash() {
    for output in [
        Output::Bytes {
            bytes: Vec::new(),
            paid: 0,
        },
        Output::Hash(Blake2b::<U32>::new()),
    ] {
        let meter = Meter {
            used: Cell::new([0; 3]),
            limit: Some([0; 3]),
        };
        let mut writer = Writer {
            meter: &meter,
            output,
            error: None,
        };
        assert!(writer.write_all(&[1, 2, 3]).is_err());
        match writer.output {
            Output::Bytes { bytes, paid } => {
                assert!(bytes.is_empty());
                assert_eq!(paid, 0);
            }
            Output::Hash(hash) => assert_eq!(hash.finalize(), Blake2b::<U32>::new().finalize()),
        }
    }
}

#[test]
fn returned_produce_clone_reserves_nested_payload_backing_before_copy() {
    let source = Produce::create(&"channel", &"datum", false)
        .mark_as_non_deterministic(vec![vec![1_u8; 17], vec![2_u8; 35]]);
    let baseline = Meter::default();
    let copied = clone_produce(&source, &baseline).unwrap();
    assert_eq!(bincode::serialize(&copied).unwrap(), bincode::serialize(&source).unwrap());
    let required = baseline.used.get();
    assert!(required.iter().all(|amount| *amount > 0));
    let clone_only = Meter::default();
    crate::rspace::native_backing::reserve(&source, &clone_only).unwrap();
    assert!(required[0] >= clone_only.used.get()[0] * 2);
    for dimension in 0..3 {
        let mut limits = required;
        limits[dimension] -= 1;
        let meter = Meter {
            used: Cell::new([0; 3]),
            limit: Some(limits),
        };
        assert!(matches!(clone_produce(&source, &meter), Err(RSpaceError::HostWorkRejected)));
        assert_eq!(bincode::serialize(&source).unwrap(), bincode::serialize(&copied).unwrap());
    }
}

#[test]
fn exact_budget_succeeds_and_one_less_rejects_each_dimension() {
    let channels = vec![3_u64, 1, 3];
    let patterns = vec![vec![2_u8], vec![], vec![2]];
    let baseline = Meter::default();
    let expected = consume(&channels, &patterns, &vec![5_u8; 257], false, &baseline).unwrap();
    let required = baseline.used.get();
    let exact = Meter {
        used: Cell::new([0; 3]),
        limit: Some(required),
    };
    assert_eq!(consume(&channels, &patterns, &vec![5_u8; 257], false, &exact).unwrap(), expected);
    for dimension in 0..3 {
        let mut limits = required;
        limits[dimension] -= 1;
        let meter = Meter {
            used: Cell::new([0; 3]),
            limit: Some(limits),
        };
        assert!(matches!(
            consume(&channels, &patterns, &vec![5_u8; 257], false, &meter),
            Err(RSpaceError::HostWorkRejected)
        ));
    }
}

#[test]
fn serializer_cannot_suppress_a_rejected_write() {
    struct Suppressed;
    impl Serialize for Suppressed {
        fn serialize<S: serde::Serializer>(
            &self,
            serializer: S,
        ) -> std::result::Result<S::Ok, S::Error> {
            use serde::ser::SerializeTuple;
            let mut tuple = serializer.serialize_tuple(2)?;
            let _ = tuple.serialize_element(&1_u8);
            let _ = tuple.serialize_element(&2_u8);
            tuple.end()
        }
    }
    let calls = Cell::new(0);
    let meter = |_: usize, _: usize, _: usize| {
        let call = calls.get();
        calls.set(call + 1);
        if call == 1 {
            Err(RSpaceError::HostWorkRejected)
        } else {
            Ok(())
        }
    };
    assert!(matches!(encode(&Suppressed, &meter), Err(RSpaceError::HostWorkRejected)));
    assert_eq!(calls.get(), 2);
}

#[test]
fn rejected_growth_preserves_prior_buffer_and_paid_capacity() {
    let meter = Meter {
        used: Cell::new([0; 3]),
        limit: Some([usize::MAX, usize::MAX, 8]),
    };
    let mut writer = Writer {
        meter: &meter,
        output: Output::Bytes {
            bytes: Vec::new(),
            paid: 0,
        },
        error: None,
    };
    writer.write_all(&[1; 8]).unwrap();
    assert!(writer.write_all(&[2]).is_err());
    let Output::Bytes { bytes, paid } = writer.output else {
        panic!("byte writer changed kind")
    };
    assert_eq!(bytes, vec![1; 8]);
    assert_eq!(paid, 8);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-S1 (D-C2a, DR-96): `consume_keys` returns the source of `consume`
    /// and one key per channel in channel order, repeats included. Its only
    /// extra charge is the key vector and one 32-byte copy per key.
    #[test]
    fn consume_keys_follow_channel_order(
        channels in prop::collection::vec("[a-c]{0,3}", 1..6),
        patterns in prop::collection::vec("[p-r]{0,3}", 1..6),
        persistent in any::<bool>(),
    ) {
        let plain = Meter::default();
        let source = consume(&channels, &patterns, &"body", persistent, &plain).unwrap();
        let keyed = Meter::default();
        let (keyed_source, keys) =
            consume_keys(&channels, &patterns, &"body", persistent, &keyed).unwrap();
        prop_assert_eq!(&keyed_source, &source);
        prop_assert_eq!(&source, &Consume::create(&channels, &patterns, &"body", persistent));
        prop_assert_eq!(keys.len(), channels.len());
        for (key, channel) in keys.iter().zip(&channels) {
            prop_assert_eq!(*key, channel_key(channel, &Meter::default()).unwrap());
        }
        let [plain_operations, plain_scanned, plain_backing] = plain.used.get();
        let [operations, scanned, backing] = keyed.used.get();
        let count = channels.len();
        let vector_bytes = count * size_of::<StoreKey>();
        prop_assert_eq!(operations - plain_operations, count + 1 + count);
        prop_assert_eq!(scanned - plain_scanned, vector_bytes + 32 * count);
        prop_assert_eq!(backing - plain_backing, vector_bytes);
    }

    /// D-S1 (D-C2a, DR-96): a channel key is the channel's history digest.
    #[test]
    fn channel_key_equals_hash(channel in "[a-z]{0,40}") {
        let key = channel_key(&channel, &Meter::default()).unwrap();
        let digest = hash(&channel, &Meter::default()).unwrap();
        prop_assert_eq!(key.0.as_slice(), digest.0.as_slice());
        prop_assert_eq!(key, StoreKey::from_digest(&digest, &Meter::default()).unwrap());
    }
}

/// D-S1 (D-C2a, DR-96): a group key hashes the domain separator and the
/// channel keys in channel order, so a reordered or repeated group has a
/// different key; join groups keep their positions.
#[test]
fn group_key_preserves_order() {
    let free = Meter::default();
    let a = channel_key(&"a", &free).unwrap();
    let b = channel_key(&"b", &free).unwrap();
    let ab = group_key(&[a, b], &free).unwrap();
    assert_ne!(ab, group_key(&[b, a], &free).unwrap());
    assert_ne!(group_key(&[a], &free).unwrap(), group_key(&[a, a], &free).unwrap());
    let mut reference = Blake2b::<U32>::new();
    reference.update(GROUP_KEY_DOMAIN);
    reference.update(a.0);
    reference.update(b.0);
    assert_eq!(ab.0.as_slice(), &reference.finalize()[..]);
    let built = GroupKeys::build(&["a", "b"], &free).unwrap();
    assert_eq!(built.channels, vec![a, b]);
    assert_eq!(built.group, ab);
    let operation = OperationKeys::build(&[vec!["b", "a"], vec!["a", "b"]], &free).unwrap();
    assert_eq!(operation.groups.len(), 2);
    assert_eq!(operation.groups[0].channels, vec![b, a]);
    assert_eq!(operation.groups[1].group, ab);
    assert_ne!(operation.groups[0].group, operation.groups[1].group);
}
