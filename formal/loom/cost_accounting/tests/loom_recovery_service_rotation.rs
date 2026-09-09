#[path = "../../../../casper/src/rust/engine/recovery_service_rotation.rs"]
mod recovery_service_rotation;

use std::collections::VecDeque;
use std::task::Poll;

use loom::sync::{Arc, Mutex};
use loom::thread;
use recovery_service_rotation::ServiceRotation;

struct Input {
    values: VecDeque<u8>,
    producer_open: bool,
    observed_open: bool,
}

#[test]
fn concurrent_inputs_drain_in_order_and_close_with_a_continuously_ready_timer() {
    loom::model(|| {
        let inputs: Vec<_> = (0..2)
            .map(|_| {
                Arc::new(Mutex::new(Input {
                    values: VecDeque::new(),
                    producer_open: true,
                    observed_open: true,
                }))
            })
            .collect();
        let writers: Vec<_> = inputs
            .iter()
            .map(|input| {
                let input = Arc::clone(input);
                thread::spawn(move || {
                    input.lock().unwrap().values.push_back(0);
                    let mut input = input.lock().unwrap();
                    input.values.push_back(1);
                    input.producer_open = false;
                })
            })
            .collect();
        let mut rotation = ServiceRotation::<3>::default();
        let mut seen = [Vec::new(), Vec::new()];
        let mut closures = [0; 2];
        let mut service = || {
            let event = rotation.poll(|lane| {
                if lane == 2 {
                    return Poll::Ready((lane, None));
                }
                let mut input = inputs[lane].lock().unwrap();
                if !input.observed_open {
                    return Poll::Pending;
                }
                if let Some(value) = input.values.pop_front() {
                    Poll::Ready((lane, Some(value)))
                } else if !input.producer_open {
                    input.observed_open = false;
                    Poll::Ready((lane, None))
                } else {
                    Poll::Pending
                }
            });
            match event {
                Poll::Ready((lane, Some(value))) => seen[lane].push(value),
                Poll::Ready((lane, None)) if lane < 2 => closures[lane] += 1,
                Poll::Ready(_) => {}
                Poll::Pending => panic!("the timer is continuously ready"),
            }
        };
        service();
        service();
        for writer in writers {
            writer.join().unwrap();
        }
        for _ in 0..9 {
            service();
        }
        assert_eq!(seen, [vec![0, 1], vec![0, 1]]);
        assert_eq!(closures, [1, 1]);
        for input in inputs {
            assert!(!input.lock().unwrap().observed_open);
        }
    });
}
