use std::sync::atomic::{AtomicUsize, Ordering};

static CLONES: AtomicUsize = AtomicUsize::new(0);
static COPIED_BYTES: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, PartialEq, Eq)]
struct Payload(Vec<u8>);

impl Clone for Payload {
    fn clone(&self) -> Self {
        CLONES.fetch_add(1, Ordering::Relaxed);
        COPIED_BYTES.fetch_add(self.0.len(), Ordering::Relaxed);
        Self(self.0.clone())
    }
}

fn reset_counts() {
    CLONES.store(0, Ordering::Relaxed);
    COPIED_BYTES.store(0, Ordering::Relaxed);
}

fn counts() -> (usize, usize) {
    (
        CLONES.load(Ordering::Relaxed),
        COPIED_BYTES.load(Ordering::Relaxed),
    )
}

fn report_copies(label: &str, before: (usize, usize), after: (usize, usize)) {
    println!(
        "{label}: existing clones={} copied_bytes={}; moved clones={} copied_bytes={}",
        before.0, before.1, after.0, after.1
    );
}

fn groups() -> [Vec<Payload>; 3] {
    std::array::from_fn(|group| {
        (0..256)
            .map(|item| Payload(vec![(group + item) as u8; 2048]))
            .collect()
    })
}

fn check_concatenation() {
    let input = groups();
    reset_counts();
    let existing = input.concat();
    let before = counts();
    assert_eq!(before, (768, 1_572_864));

    let input = groups();
    reset_counts();
    let moved: Vec<_> = input.into_iter().flatten().collect();
    let after = counts();
    assert_eq!(after, (0, 0));
    assert_eq!(existing, moved);
    report_copies("concatenation", before, after);
}

fn existing_first(values: Vec<Option<Payload>>) -> Option<Payload> {
    match values.split_first() {
        Some((first, _)) => first.clone(),
        None => None,
    }
}

fn moved_first(values: Vec<Option<Payload>>) -> Option<Payload> {
    values.into_iter().next().flatten()
}

fn check_point_read() {
    let values = vec![Some(Payload(vec![7; 1_048_576]))];
    reset_counts();
    let existing = existing_first(values);
    let before = counts();
    assert_eq!(before, (1, 1_048_576));

    let values = vec![Some(Payload(vec![7; 1_048_576]))];
    reset_counts();
    let moved = moved_first(values);
    let after = counts();
    assert_eq!(after, (0, 0));
    assert_eq!(existing, moved);
    assert_eq!(existing_first(Vec::new()), moved_first(Vec::new()));
    assert_eq!(existing_first(vec![None]), moved_first(vec![None]));
    report_copies("point_read", before, after);
}

fn batch() -> Vec<((usize, Payload), bool)> {
    (0..256)
        .map(|id| ((id, Payload(vec![id as u8; 2048])), id % 2 == 0))
        .collect()
}

fn check_conditional_batch() {
    let input = batch();
    reset_counts();
    let existing: Vec<_> = input
        .clone()
        .into_iter()
        .filter(|(_, present)| !present)
        .map(|(pair, _)| pair)
        .collect();
    let before = counts();
    assert_eq!(before, (256, 524_288));

    let input = batch();
    reset_counts();
    let moved: Vec<_> = input
        .into_iter()
        .filter(|(_, present)| !present)
        .map(|(pair, _)| pair)
        .collect();
    let after = counts();
    assert_eq!(after, (0, 0));
    assert_eq!(existing.len(), 128);
    assert_eq!(existing, moved);
    report_copies("conditional_batch", before, after);
}

fn sort_key(value: u64, repeated_keys: bool) -> u64 {
    let mixed = value.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(17);
    if repeated_keys {
        mixed % 7
    } else {
        mixed
    }
}

fn check_sort(length: usize, repeated_keys: bool) {
    let input: Vec<_> = (0..length as u64).collect();
    let mut existing = input.clone();
    let mut existing_calls = 0;
    existing.sort_by_key(|value| {
        existing_calls += 1;
        sort_key(*value, repeated_keys)
    });

    let mut prepared_calls = 0;
    let mut prepared: Vec<_> = input
        .into_iter()
        .map(|value| {
            prepared_calls += 1;
            (sort_key(value, repeated_keys), value)
        })
        .collect();
    prepared.sort_by_key(|(key, _)| *key);
    let prepared: Vec<_> = prepared.into_iter().map(|(_, value)| value).collect();
    assert_eq!(existing, prepared);
    assert_eq!(prepared_calls, length);
    for pair in prepared.windows(2) {
        if sort_key(pair[0], repeated_keys) == sort_key(pair[1], repeated_keys) {
            assert!(pair[0] < pair[1]);
        }
    }
    println!(
        "sort n={length} repeated_keys={repeated_keys}: existing_key_calls={existing_calls} prepared_key_calls={prepared_calls}"
    );
}

fn main() {
    check_concatenation();
    check_point_read();
    check_conditional_batch();
    for length in [0, 16, 128, 1024] {
        check_sort(length, false);
        check_sort(length, true);
    }
    println!("All synthetic operation-count and value-equivalence assertions passed.");
}
