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

/// D-S2 (DR-95): a record with a tagged tree set, and its untagged twin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct TaggedSet {
    before: u64,
    #[serde(deserialize_with = "tree_set")]
    peeks: BTreeSet<i32>,
    after: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct UntaggedSet {
    before: u64,
    peeks: BTreeSet<i32>,
    after: String,
}

fn history_options() -> impl Options {
    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-S2 (DR-95): the tree-set tag is transparent on the wire in every
    /// bincode configuration that decodes these records: the tagged and
    /// untagged types encode to the same bytes, and the tagged type decodes
    /// those bytes to the same value, also through the metered decoder.
    #[test]
    fn tree_set_tag_is_wire_transparent(
        before in any::<u64>(),
        peeks in prop::collection::btree_set(any::<i32>(), 0..16),
        after in "[a-z]{0,12}",
    ) {
        let tagged = TaggedSet { before, peeks: peeks.clone(), after: after.clone() };
        let untagged = UntaggedSet { before, peeks, after };
        let default_bytes = bincode::serialize(&untagged).unwrap();
        prop_assert_eq!(&bincode::serialize(&tagged).unwrap(), &default_bytes);
        prop_assert_eq!(&bincode::deserialize::<TaggedSet>(&default_bytes).unwrap(), &tagged);
        let history_bytes = history_options().serialize(&untagged).unwrap();
        prop_assert_eq!(&history_options().serialize(&tagged).unwrap(), &history_bytes);
        prop_assert_eq!(&history_options().deserialize::<TaggedSet>(&history_bytes).unwrap(), &tagged);
        prop_assert_eq!(
            &decode_record::<TaggedSet, _>(&history_bytes, &Meter::new(usize::MAX)).unwrap(),
            &tagged
        );
        let varint_bytes = bincode::DefaultOptions::new().serialize(&untagged).unwrap();
        prop_assert_eq!(
            &bincode::DefaultOptions::new().deserialize::<TaggedSet>(&varint_bytes).unwrap(),
            &tagged
        );
    }
}

/// D-S2 (DR-95): the tag is not a node of its own: the metered decoder makes
/// the same reservations for the tagged and the untagged type.
#[test]
fn tree_set_tag_leaves_the_decode_charge_unchanged() {
    let untagged = UntaggedSet {
        before: 7,
        peeks: (0..9).collect(),
        after: "peeks".to_owned(),
    };
    let bytes = history_options().serialize(&untagged).unwrap();
    let tagged_meter = Meter::new(usize::MAX);
    let untagged_meter = Meter::new(usize::MAX);
    decode_record::<TaggedSet, _>(&bytes, &tagged_meter).unwrap();
    decode_record::<UntaggedSet, _>(&bytes, &untagged_meter).unwrap();
    assert_eq!(tagged_meter.calls.get(), untagged_meter.calls.get());
    assert_eq!(tagged_meter.bytes.get(), untagged_meter.bytes.get());
}

/// D-S2 (DR-95): history records of waiting continuations decode in both
/// modes to the values that plain bincode decodes, with the tagged peek set,
/// and History mode reserves backing for every byte that it allocates.
#[test]
fn history_decode_values_equal_bincode() {
    use crate::rspace::internal::WaitingContinuation;

    for peeks in [BTreeSet::new(), [0].into(), [0, 2, 5, 9].into()] {
        let continuation = WaitingContinuation::create(
            &vec!["channel-a".to_owned(), "channel-b".to_owned()],
            &vec!["pattern".to_owned(), "other".to_owned()],
            &"body".to_owned(),
            true,
            peeks,
        );
        let bytes = history_options().serialize(&continuation).unwrap();
        let expected: WaitingContinuation<String, String> = bincode::deserialize(&bytes).unwrap();
        assert_eq!(expected, continuation);
        let decoded: WaitingContinuation<String, String> =
            decode_record(&bytes, &Meter::new(usize::MAX)).unwrap();
        assert_eq!(decoded, continuation);
        let meter = Meter::new(usize::MAX);
        let (decoded, allocated) = measure(|| {
            decode_history_record::<WaitingContinuation<String, String>, _>(&bytes, &meter)
        });
        assert_eq!(decoded.unwrap(), continuation);
        assert!(allocated <= meter.bytes.get());
    }
}

/// A meter that records every charge.
struct Recorder {
    charges: std::cell::RefCell<Vec<NativeReadCharge>>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            charges: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl NativeReadMeter for Recorder {
    type Error = usize;

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), usize> {
        self.charges.borrow_mut().push(charge);
        Ok(())
    }
}

/// The bytes that every decode pre-charges for bincode's error box.
fn error_box() -> usize { size_of::<bincode::ErrorKind>() + 64 }

macro_rules! closed_test_types {
    ($($ty:ty),* $(,)?) => {
        // SAFETY: test records with plain derives over vectors, arrays,
        // strings, maps, tagged sets and scalars.
        $(unsafe impl ClosedDecode for $ty {})*
    };
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Element296 {
    head: [u64; 32],
    tail: [u64; 5],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Element600 {
    first: [u64; 32],
    second: [u64; 32],
    rest: [u64; 11],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Element1025 {
    rows: [[u8; 32]; 32],
    last: u8,
}

impl Default for Element1025 {
    fn default() -> Self {
        Self {
            rows: [[3; 32]; 32],
            last: 9,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Pair {
    left: u32,
    right: Vec<u16>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Mixed {
    names: Vec<String>,
    rows: BTreeMap<u64, Vec<u8>>,
    #[serde(deserialize_with = "tree_set")]
    peeks: BTreeSet<i32>,
    note: Option<String>,
}

closed_test_types!(TaggedSet, Element296, Element600, Element1025, Pair, Mixed, Choice);

/// D-S2 (DR-95): decoding a vector of `count` elements in History mode
/// reserves exactly the bytes that the vector's buffers allocate (the error
/// box aside), for every element size class of `RawVec`.
fn growth_matches<T>(count: usize)
where T: Clone + Default + PartialEq + std::fmt::Debug + Serialize + DeserializeOwned + ClosedDecode
{
    let values = vec![T::default(); count];
    let bytes = history_options().serialize(&values).unwrap();
    let meter = Meter::new(usize::MAX);
    let (decoded, allocated) = measure(|| decode_history_record::<Vec<T>, _>(&bytes, &meter));
    assert_eq!(decoded.unwrap(), values);
    assert_eq!(
        allocated,
        meter.bytes.get() - error_box(),
        "{} elements of {} bytes",
        count,
        size_of::<T>()
    );
}

#[test]
fn vec_growth_model_matches_std_allocations() {
    assert_eq!(size_of::<Element296>(), 296);
    assert_eq!(size_of::<Element600>(), 600);
    assert_eq!(size_of::<Element1025>(), 1025);
    for count in 0..=40 {
        growth_matches::<u8>(count);
        growth_matches::<u32>(count);
        growth_matches::<[u64; 3]>(count);
        growth_matches::<Element296>(count);
        growth_matches::<Element600>(count);
        growth_matches::<Element1025>(count);
    }
}

/// The node charges of a History decode of `value`: the charges of one
/// operation and no backing. The values below hold no empty string, whose
/// exact-length reservation would also match.
fn history_node_charges<T>(value: &T) -> usize
where T: std::fmt::Debug + PartialEq + Serialize + DeserializeOwned + ClosedDecode {
    let bytes = history_options().serialize(value).unwrap();
    let history = Recorder::new();
    assert_eq!(&decode_history_record::<T, _>(&bytes, &history).unwrap(), value);
    let charges = history.charges.borrow();
    charges
        .iter()
        .filter(|charge| charge.backing_bytes == 0 && charge.operations == 1)
        .count()
}

/// D-S2 (DR-95): History mode charges each node once (one operation and its
/// size, no backing); the legacy rule charged each node at both hooks.
#[test]
fn history_decode_charges_each_node_once() {
    let value = Pair {
        left: 7,
        right: vec![1, 2],
    };
    // The record, its two fields and the two vector elements.
    let nodes = history_node_charges(&value);
    assert_eq!(nodes, 5);
    // An enum is one node, and its variant identifier is another. The body of
    // a tuple or struct variant is not a node of its own: its fields are.
    assert_eq!(history_node_charges(&Choice::Empty), 2);
    assert_eq!(history_node_charges(&Choice::One("ab".to_owned())), 3);
    assert_eq!(history_node_charges(&Choice::Pair(7, vec![1, 2])), 6);
    assert_eq!(
        history_node_charges(&Choice::Named {
            flag: true,
            items: vec![Some("x".to_owned()), None],
        }),
        7
    );
    let bytes = history_options().serialize(&value).unwrap();
    let legacy = Recorder::new();
    assert_eq!(decode_record::<Pair, _>(&bytes, &legacy).unwrap(), value);
    assert!(legacy.charges.borrow().len() > 2 * nodes - 1);
}

/// D-S2 (DR-95): History mode reserves each allocation before it happens:
/// whatever reservation the meter rejects, the bytes allocated up to that
/// point are covered by the reservations accepted before it.
#[test]
fn every_history_reservation_cut_covers_allocations() {
    let value = Mixed {
        names: (0..9).map(|index| format!("name-{index}")).collect(),
        rows: (0..7)
            .map(|key| (key, vec![key as u8; 3 + key as usize]))
            .collect(),
        peeks: (0..13).collect(),
        note: Some("a note".to_owned()),
    };
    let bytes = history_options().serialize(&value).unwrap();
    let total = {
        let meter = Meter::new(usize::MAX);
        assert_eq!(decode_history_record::<Mixed, _>(&bytes, &meter).unwrap(), value);
        meter.calls.get()
    };
    for cut in 0..=total {
        let meter = Meter::new(cut);
        let (result, allocated) = measure(|| decode_history_record::<Mixed, _>(&bytes, &meter));
        assert!(
            allocated <= meter.bytes.get(),
            "cut {cut}: allocated {allocated} reserved {}",
            meter.bytes.get()
        );
        if cut < total {
            assert!(matches!(result, Err(NativeReadError::Host(_))), "cut {cut}");
        } else {
            assert_eq!(result.unwrap(), value);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// D-S2 (DR-95): History mode decodes the values that bincode decodes and
    /// reserves backing for every allocation of generated mixed records.
    #[test]
    fn history_records_values_and_allocations(
        names in prop::collection::vec("[a-z]{0,24}", 0..20),
        rows in prop::collection::btree_map(any::<u64>(), prop::collection::vec(any::<u8>(), 0..40), 0..12),
        peeks in prop::collection::btree_set(any::<i32>(), 0..24),
        note in prop::option::of("[a-z]{0,16}"),
    ) {
        let value = Mixed { names, rows, peeks, note };
        let bytes = history_options().serialize(&value).unwrap();
        let meter = Meter::new(usize::MAX);
        let (decoded, allocated) = measure(|| decode_history_record::<Mixed, _>(&bytes, &meter));
        prop_assert_eq!(decoded.unwrap(), value.clone());
        prop_assert!(allocated <= meter.bytes.get());
        let legacy = Meter::new(usize::MAX);
        prop_assert_eq!(decode_record::<Mixed, _>(&bytes, &legacy).unwrap(), value);
        prop_assert!(meter.bytes.get() <= legacy.bytes.get());
    }
}
