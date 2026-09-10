use std::task::Poll;

#[derive(Default)]
pub(super) struct ServiceRotation<const LANES: usize> {
    first: usize,
}

impl<const LANES: usize> ServiceRotation<LANES> {
    pub(super) fn poll<T>(&mut self, mut poll_lane: impl FnMut(usize) -> Poll<T>) -> Poll<T> {
        assert!(LANES > 0);
        let mut lane = self.first;
        for _ in 0..LANES {
            let result = poll_lane(lane);
            lane = if lane == LANES - 1 { 0 } else { lane + 1 };
            if result.is_ready() {
                self.first = lane;
                return result;
            }
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn check_history<const LANES: usize>(history: &[u64]) {
        let mut rotation = ServiceRotation::<LANES>::default();
        let mut first = 0;
        let mut debt = [0; LANES];
        for mask in history {
            let ready: [bool; LANES] = std::array::from_fn(|lane| mask & (1 << lane) != 0);
            let mut visited = Vec::new();
            let actual = rotation.poll(|lane| {
                visited.push(lane);
                if ready[lane] {
                    Poll::Ready(lane)
                } else {
                    Poll::Pending
                }
            });
            let ordered: Vec<_> = (first..LANES).chain(0..first).collect();
            let expected = ordered.iter().copied().find(|lane| ready[*lane]);
            assert_eq!(actual, expected.map_or(Poll::Pending, Poll::Ready));
            let visited_count = expected.map_or(LANES, |lane| {
                ordered.iter().position(|value| *value == lane).unwrap() + 1
            });
            assert_eq!(visited, ordered[..visited_count]);
            if let Some(selected) = expected {
                first = (selected + 1) % LANES;
                for lane in 0..LANES {
                    if lane == selected || !ready[lane] {
                        debt[lane] = 0;
                    } else {
                        debt[lane] += 1;
                    }
                    let rank = (lane + LANES - first) % LANES;
                    assert!(debt[lane] < LANES);
                    if ready[lane] {
                        assert!(debt[lane] + rank < LANES);
                    }
                }
            } else {
                debt.fill(0);
            }
        }
    }

    #[test]
    fn every_three_lane_readiness_transition_preserves_the_service_bound() {
        for initial in 0..3 {
            for sequence in 0..4096 {
                let mut history = vec![1 << initial];
                history.extend((0..4).map(|step| (sequence >> (3 * step)) & 7));
                check_history::<3>(&history);
            }
        }
    }

    #[test]
    fn continuous_readiness_rotates_all_lanes() {
        let mut rotation = ServiceRotation::<3>::default();
        for turn in 0..300 {
            assert_eq!(rotation.poll(Poll::Ready), Poll::Ready(turn % 3));
        }
    }

    proptest! {
        #[test]
        fn generated_ready_pending_histories_refine_the_rank_proof(
            history in prop::collection::vec(any::<u64>(), 0..1000),
        ) {
            check_history::<1>(&history);
            check_history::<2>(&history);
            check_history::<3>(&history);
            check_history::<17>(&history);
            check_history::<64>(&history);
        }
    }
}
