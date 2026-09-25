use std::cmp::Ordering;

pub fn sort<T, E>(
    values: &mut [T],
    compare: impl Fn(&T, &T) -> Result<Ordering, E>,
) -> Result<(), E> {
    fn sift<T, E>(
        values: &mut [T],
        mut root: usize,
        compare: &impl Fn(&T, &T) -> Result<Ordering, E>,
    ) -> Result<(), E> {
        while root < values.len() / 2 {
            let mut child = root * 2 + 1;
            if child + 1 < values.len() && compare(&values[child], &values[child + 1])?.is_lt() {
                child += 1;
            }
            if !compare(&values[root], &values[child])?.is_lt() {
                break;
            }
            values.swap(root, child);
            root = child;
        }
        Ok(())
    }
    for root in (0..values.len() / 2).rev() {
        sift(values, root, &compare)?;
    }
    for end in (1..values.len()).rev() {
        values.swap(0, end);
        sift(&mut values[..end], 0, &compare)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn generated_sort_matches_reference(mut values in prop::collection::vec(any::<i32>(), 0..256)) {
            let mut expected = values.clone();
            expected.sort_unstable();
            sort(&mut values, |a, b| Ok::<_, ()>(a.cmp(b))).unwrap();
            prop_assert_eq!(values, expected);
        }

        #[test]
        fn comparator_failure_preserves_all_elements(
            mut values in prop::collection::vec(any::<i32>(), 0..128),
            accepted in 0_usize..1024,
        ) {
            let mut expected = values.clone();
            expected.sort_unstable();
            let calls = Cell::new(0);
            let result = sort(&mut values, |a, b| {
                let call = calls.get();
                calls.set(call + 1);
                if call == accepted { Err(call) } else { Ok(a.cmp(b)) }
            });
            if let Err(call) = result {
                prop_assert_eq!(call, accepted);
                prop_assert_eq!(calls.get(), accepted + 1);
            }
            values.sort_unstable();
            prop_assert_eq!(values, expected);
        }
    }

    #[test]
    fn sort_does_not_require_clone() {
        struct Value(u32);
        let mut values = [Value(3), Value(1), Value(2), Value(1)];
        sort(&mut values, |a, b| Ok::<_, ()>(a.0.cmp(&b.0))).unwrap();
        assert_eq!(values.map(|value| value.0), [1, 1, 2, 3]);
    }
}
