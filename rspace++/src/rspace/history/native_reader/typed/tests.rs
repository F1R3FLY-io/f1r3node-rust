use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use proptest::prelude::*;
use serde::{Deserialize, Serialize};

use super::*;
use crate::rspace::history::native_reader::tests::allocations::measure;

struct Meter {
    remaining: Cell<usize>,
    calls: Cell<usize>,
    bytes: Cell<usize>,
}

impl Meter {
    fn new(remaining: usize) -> Self {
        Self {
            remaining: Cell::new(remaining),
            calls: Cell::new(0),
            bytes: Cell::new(0),
        }
    }
}

impl NativeReadMeter for Meter {
    type Error = usize;

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), usize> {
        if self.remaining.get() == 0 {
            return Err(self.calls.get());
        }
        self.remaining.set(self.remaining.get() - 1);
        self.calls.set(self.calls.get() + 1);
        self.bytes.set(self.bytes.get() + charge.backing_bytes);
        Ok(())
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Choice {
    Empty,
    One(String),
    Pair(u64, Vec<u8>),
    Named {
        flag: bool,
        items: Vec<Option<String>>,
    },
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Record {
    choices: Vec<Choice>,
    names: BTreeSet<String>,
    rows: BTreeMap<String, Vec<u64>>,
    index: HashMap<u64, String>,
    boxed: Box<(String, u64)>,
}

fn record() -> Record {
    Record {
        choices: vec![
            Choice::Empty,
            Choice::One("payload".repeat(128)),
            Choice::Pair(42, vec![5; 64]),
            Choice::Named {
                flag: true,
                items: vec![None, Some("nested".to_owned())],
            },
        ],
        names: ["a".to_owned(), "b".to_owned()].into(),
        rows: [("row".to_owned(), vec![1, 2, 3])].into(),
        index: [(42, "answer".to_owned())].into(),
        boxed: Box::new(("boxed".to_owned(), 7)),
    }
}

#[test]
fn derived_records_keep_the_legacy_wire_and_fit_reserved_backing() {
    let expected = record();
    let bytes = bincode::serialize(&expected).unwrap();
    let meter = Meter::new(usize::MAX);
    let (actual, allocations) = measure(|| decode_record::<Record, _>(&bytes, &meter));
    assert_eq!(actual.unwrap(), expected);
    assert!(
        allocations <= meter.bytes.get(),
        "allocated={allocations}, paid={}",
        meter.bytes.get()
    );
    let mut with_trailer = bytes.clone();
    with_trailer.extend_from_slice(&[9, 8, 7]);
    assert_eq!(bincode::deserialize::<Record>(&with_trailer).unwrap(), expected);
    assert_eq!(
        decode_record::<Record, _>(&with_trailer, &Meter::new(usize::MAX)).unwrap(),
        expected
    );
}

#[test]
fn every_reservation_cut_preserves_the_host_error() {
    let bytes = bincode::serialize(&record()).unwrap();
    let meter = Meter::new(usize::MAX);
    decode_record::<Record, _>(&bytes, &meter).unwrap();
    for accepted in 0..meter.calls.get() {
        let limited = Meter::new(accepted);
        let (result, allocations) = measure(|| decode_record::<Record, _>(&bytes, &limited));
        assert!(matches!(result,
            Err(NativeReadError::Host(error)) if error == accepted));
        assert!(
            allocations <= limited.bytes.get(),
            "cut={accepted}, allocated={allocations}, paid={}",
            limited.bytes.get()
        );
    }
}

#[test]
fn every_truncated_record_preserves_wire_rejection_and_reserved_backing() {
    let bytes = bincode::serialize(&record()).unwrap();
    for end in 0..bytes.len() {
        let prefix = &bytes[..end];
        let meter = Meter::new(usize::MAX);
        let (result, allocations) = measure(|| decode_record::<Record, _>(prefix, &meter));
        assert!(bincode::deserialize::<Record>(prefix).is_err());
        assert!(matches!(result, Err(NativeReadError::Invalid(NativeReadFault::TypedRecord))));
        assert!(
            allocations <= meter.bytes.get(),
            "end={end}, allocated={allocations}, paid={}",
            meter.bytes.get()
        );
    }
}

#[test]
fn hostile_lengths_cannot_drive_unreserved_sequence_allocation() {
    let length = u64::MAX.to_le_bytes();
    for accepted in [0, 1, 5, 25] {
        assert!(decode_record::<Vec<()>, _>(&length, &Meter::new(accepted)).is_err());
    }
    assert!(matches!(
        decode_record::<Vec<String>, _>(&length, &Meter::new(usize::MAX)),
        Err(NativeReadError::Invalid(NativeReadFault::TypedRecord))
    ));
    assert!(matches!(
        decode_record::<String, _>(&length, &Meter::new(usize::MAX)),
        Err(NativeReadError::Invalid(NativeReadFault::TypedRecord))
    ));
}

#[derive(Serialize, Deserialize)]
enum Recursive {
    Done,
    Next(Box<Recursive>),
}

#[test]
fn recursive_records_stop_before_the_decode_stack_is_unbounded() {
    let bytes: Vec<_> = (0..MAX_DEPTH * 2)
        .flat_map(|_| 1_u32.to_le_bytes())
        .chain(0_u32.to_le_bytes())
        .collect();
    assert!(matches!(
        decode_record::<Recursive, _>(&bytes, &Meter::new(usize::MAX)),
        Err(NativeReadError::Invalid(NativeReadFault::Depth))
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn nested_vectors_preserve_values_and_cover_actual_allocations(
        values in prop::collection::vec(prop::collection::vec("[a-z]{0,32}", 0..12), 0..12),
    ) {
        let bytes = bincode::serialize(&values).unwrap();
        let meter = Meter::new(usize::MAX);
        let (actual, allocations) = measure(|| decode_record::<Vec<Vec<String>>, _>(&bytes, &meter));
        prop_assert_eq!(actual.unwrap(), values);
        prop_assert!(allocations <= meter.bytes.get());
    }
}
