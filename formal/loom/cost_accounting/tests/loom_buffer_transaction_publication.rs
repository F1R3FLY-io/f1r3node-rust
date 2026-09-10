use loom::sync::{Arc, Mutex};
use loom::thread;

#[path = "../../../../block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage/buffer_transaction.rs"]
mod buffer_transaction;

#[derive(Default)]
struct Projection {
    disk: [u8; 2],
    memory: [u8; 2],
}

#[test]
fn production_commit_control_preserves_atomic_publication_and_failed_writes() {
    for failures in 0u8..4 {
        let mut builder = loom::model::Builder::new();
        builder.max_threads = 3;
        builder.max_branches = 1000;
        builder.max_permutations = None;
        builder.max_duration = None;
        builder.preemption_bound = None;
        builder.checkpoint_file = None;
        builder.check(move || {
            let shared = Arc::new(Mutex::new(Projection::default()));
            let writers: Vec<_> = (0..2)
                .map(|index| {
                    let shared = shared.clone();
                    thread::spawn(move || {
                        let mut guard = shared.lock().unwrap();
                        let mut next = guard.memory;
                        next[index] = 1;
                        let Projection { disk, memory } = &mut *guard;
                        let fail = failures & (1 << index) != 0;
                        let result = buffer_transaction::commit_then_publish(
                            next,
                            |plan| {
                                thread::yield_now();
                                if fail {
                                    return Err(());
                                }
                                *disk = *plan;
                                thread::yield_now();
                                Ok(())
                            },
                            |plan| *memory = plan,
                        );
                        assert_eq!(result.is_err(), fail);
                        assert_eq!(disk, memory);
                    })
                })
                .collect();
            {
                let observed = shared.lock().unwrap();
                assert_eq!(observed.disk, observed.memory);
            }
            for writer in writers {
                writer.join().unwrap();
            }
            let final_state = shared.lock().unwrap();
            assert_eq!(final_state.disk, final_state.memory);
            for index in 0..2 {
                assert_eq!(
                    final_state.disk[index],
                    u8::from(failures & (1 << index) == 0)
                );
            }
        });
    }
}

#[test]
#[should_panic(expected = "unlocked reader observed incomplete publication")]
fn releasing_the_guard_between_commit_and_publication_is_an_unsafe_control() {
    loom::model(|| {
        let shared = Arc::new(Mutex::new(Projection::default()));
        let writer_state = shared.clone();
        let writer = thread::spawn(move || {
            buffer_transaction::commit_then_publish(
                [1, 1],
                |plan| {
                    writer_state.lock().unwrap().disk = *plan;
                    thread::yield_now();
                    Ok::<_, ()>(())
                },
                |plan| writer_state.lock().unwrap().memory = plan,
            )
            .unwrap();
        });
        let (disk, memory) = {
            let observed = shared.lock().unwrap();
            (observed.disk, observed.memory)
        };
        assert_eq!(
            disk, memory,
            "unlocked reader observed incomplete publication"
        );
        writer.join().unwrap();
    });
}
