use std::cell::Cell;

use proptest::prelude::*;

struct PublicationGuard<'a>(&'a Cell<bool>);

impl Drop for PublicationGuard<'_> {
    fn drop(&mut self) { self.0.set(false); }
}

#[path = "../../../../block-storage/src/rust/dag/admitted_metadata.rs"]
mod admitted_metadata;
#[path = "../../../../casper/src/rust/util/proto_util/dependency_readiness.rs"]
mod dependency_readiness;

proptest! {
    #[test]
    fn publication_guard_covers_reads_writes_errors_and_early_return(
        admitted in any::<bool>(),
        read_error in any::<bool>(),
        write_error in any::<bool>(),
    ) {
        let live = Cell::new(true);
        let writes = Cell::new(0);
        let result = admitted_metadata::publish_if_unadmitted(
            PublicationGuard(&live),
            || {
                assert!(live.get());
                if read_error { Err("read") } else { Ok(admitted) }
            },
            || {
                assert!(live.get());
                writes.set(writes.get() + 1);
                if write_error { Err("write") } else { Ok(17) }
            },
        );
        let expected = if read_error { Err("read") }
                       else if admitted { Ok(None) }
                       else if write_error { Err("write") }
                       else { Ok(Some(17)) };
        prop_assert_eq!(result, expected);
        prop_assert_eq!(writes.get(), usize::from(!read_error && !admitted));
        prop_assert!(!live.get());
    }

    #[test]
    fn all_read_histories_preserve_errors_and_complete_missing_checks(
        observations in prop::collection::vec((any::<bool>(), 0_u8..3), 0..512)
    ) {
        let expected: Vec<_> = observations.iter().enumerate().map(|(i, &(visible, row))| {
            match (visible, row) {
                (false, _) | (true, 0) => Ok(false),
                (true, 1) => Ok(true),
                _ => Err(i),
            }
        }).collect();
        let first_fault = expected.iter().position(Result::is_err);
        let expected_result = match first_fault {
            Some(i) => Err(i),
            None => Ok(expected.iter().all(|result| *result == Ok(true))),
        };
        let mut read_count = 0;
        let mut visits = 0;
        let actual = dependency_readiness::all_observed(observations.iter().enumerate().map(|(i, &(visible, row))| {
            visits += 1;
            admitted_metadata::metadata_present(visible, || {
                read_count += 1;
                match row { 0 => Ok(false), 1 => Ok(true), _ => Err(i) }
            })
        }));
        prop_assert_eq!(actual, expected_result);
        let examined = first_fault.map_or(observations.len(), |i| i + 1);
        prop_assert_eq!(visits, examined);
        prop_assert_eq!(read_count, observations[..examined].iter().filter(|entry| entry.0).count());
    }

    #[test]
    fn source_group_partition_does_not_change_readiness(
        observations in prop::collection::vec(0_u8..3, 0..512),
        group_size in 1_usize..64,
    ) {
        let decode = |value: &u8| match value { 0 => Ok(false), 1 => Ok(true), _ => Err(()) };
        let flat = dependency_readiness::all_observed(observations.iter().map(decode));
        let grouped = dependency_readiness::all_observed(observations.chunks(group_size)
            .map(|group| dependency_readiness::all_observed(group.iter().map(decode))));
        prop_assert_eq!(flat, grouped);
    }
}

#[test]
fn missing_does_not_suppress_later_fault() {
    assert_eq!(
        dependency_readiness::all_observed([Ok(false), Err("read failed")]),
        Err("read failed")
    );
    assert_eq!(
        admitted_metadata::metadata_present(false, || -> Result<bool, ()> {
            panic!("unpublished row read")
        }),
        Ok(false)
    );
}
