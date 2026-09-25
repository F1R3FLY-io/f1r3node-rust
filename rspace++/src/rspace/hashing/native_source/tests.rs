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
