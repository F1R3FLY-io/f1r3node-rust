use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use pathmap::PathMap;
use pathmap::zipper::{ZipperMoving, ZipperValues, ZipperWriting};
use proptest::prelude::*;

use super::{
    BarrierProvider, DisposeValue, DrainObserver, SyntheticBarrier, TeardownBarrier,
    drain_pathmap_observed, drain_pathmap_with_provider, retire_pathmap,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Handle {
    arena: u64,
    slot: u64,
    synthetic: bool,
}

fn real(slot: u64) -> Handle {
    Handle {
        arena: 73,
        slot,
        synthetic: false,
    }
}

#[derive(Default)]
struct Provider {
    next: Cell<u64>,
    live: RefCell<BTreeSet<u64>>,
    retired: Cell<u64>,
}

impl BarrierProvider<Handle> for Provider {
    fn make_barrier(&self) -> Handle {
        let slot = self.next.get();
        self.next.set(slot + 1);
        assert!(self.live.borrow_mut().insert(slot));
        Handle {
            arena: 73,
            slot,
            synthetic: true,
        }
    }

    fn discard_barrier(&self, value: Handle) {
        assert_eq!(value.arena, 73);
        assert!(value.synthetic);
        assert!(self.live.borrow_mut().remove(&value.slot));
        self.retired.set(self.retired.get() + 1);
    }
}

impl Provider {
    fn assert_balanced(&self) {
        assert!(self.live.borrow().is_empty());
        assert_eq!(self.next.get(), self.retired.get());
    }
}

struct StaticProvider;

impl TeardownBarrier<Handle> for StaticProvider {
    fn make_barrier() -> Handle {
        Handle {
            arena: 73,
            slot: 0,
            synthetic: true,
        }
    }
    fn discard_barrier(value: Handle) {
        assert!(value.synthetic);
        assert_eq!(value.arena, 73);
    }
}

#[derive(Clone, Default)]
struct Disposer(Arc<Mutex<Vec<Handle>>>);

impl DisposeValue<Handle> for Disposer {
    fn dispose(&self, value: Handle) {
        assert_eq!(value.arena, 73);
        assert!(!value.synthetic);
        self.0.lock().unwrap().push(value);
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Removed,
    Pruned(usize),
}

#[derive(Default)]
struct Observer(Vec<Event>);

impl DrainObserver for Observer {
    fn removed_value(&mut self) { self.0.push(Event::Removed); }
    fn pruned(&mut self, width: usize) {
        assert_eq!(width, 1);
        self.0.push(Event::Pruned(width));
    }
}

#[test]
fn stateful_factory_is_lazy_and_uninstalled_barriers_are_retired() {
    let provider = Provider::default();
    for rooted in [false, true] {
        let mut map = PathMap::new();
        if rooted {
            map.set_val_at([], real(9));
        }
        assert_eq!(retire_pathmap(map, &Disposer::default(), &provider), usize::from(rooted));
        assert_eq!(provider.next.get(), 0);
    }
    drop(SyntheticBarrier::new(&provider));
    assert_eq!(provider.next.get(), 1);
    provider.assert_balanced();
}

#[test]
fn stateful_retirement_preserves_cow_sibling_and_value_free_topology() {
    let mut map = PathMap::new();
    map.set_val_at([], real(0));
    for byte in 0..=255 {
        map.set_val_at([byte, 0], real(u64::from(byte) + 1));
    }
    map.create_path([31, 1, 2, 3]);
    let sibling = map.clone();
    let provider = Provider::default();
    let dispose = Disposer::default();
    assert_eq!(retire_pathmap(map, &dispose, &provider), 257);
    assert_eq!(dispose.0.lock().unwrap().len(), 257);
    provider.assert_balanced();
    assert_eq!(provider.next.get(), 0);
    assert_eq!(sibling.get_val_at([]), Some(&real(0)));
    for byte in 0..=255 {
        assert_eq!(sibling.get_val_at([byte, 0]), Some(&real(u64::from(byte) + 1)));
    }
    assert!(sibling.path_exists_at([31, 1, 2, 3]));
    assert_eq!(retire_pathmap(sibling, &dispose, &provider), 257);
    assert_eq!(dispose.0.lock().unwrap().len(), 514);
    provider.assert_balanced();
    assert_eq!(provider.next.get(), 0);
}

#[test]
fn stateful_deep_branching_and_value_free_retirement_use_256k_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            const DEPTH: usize = 8192;
            for values in [false, true] {
                let mut map = PathMap::new();
                {
                    let mut cursor = map.write_zipper();
                    cursor.set_val(real(0));
                    for depth in 0..DEPTH {
                        cursor.descend_to_byte(1);
                        cursor.set_val(real(depth as u64 + 1));
                        cursor.ascend_byte();
                        cursor.descend_to_byte(0);
                        cursor.set_val(real(depth as u64 + 1));
                    }
                    cursor.reset();
                }
                if !values {
                    let mut cursor = map.write_zipper();
                    loop {
                        if cursor.val().is_some() {
                            super::remove_value_preserving_path(&mut cursor);
                        }
                        if !super::advance_to_next_value(&mut cursor) {
                            break;
                        }
                    }
                    cursor.reset();
                }
                let provider = Provider::default();
                let dispose = Disposer::default();
                let expected = if values { DEPTH * 2 + 1 } else { 0 };
                assert_eq!(retire_pathmap(map, &dispose, &provider), expected);
                assert_eq!(dispose.0.lock().unwrap().len(), expected);
                provider.assert_balanced();
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[cfg(unix)]
#[test]
fn stateful_retirement_panics_fail_stop_without_raw_unwind() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};

    const MODE: &str = "F1R3_STATEFUL_RETIREMENT_PANIC";
    #[derive(Clone)]
    struct PanicPolicy(String);
    impl DisposeValue<Handle> for PanicPolicy {
        fn dispose(&self, _value: Handle) {
            assert_ne!(self.0, "dispose", "stateful retirement injected panic");
        }
    }
    impl BarrierProvider<Handle> for PanicPolicy {
        fn make_barrier(&self) -> Handle {
            assert_ne!(self.0, "make", "stateful retirement injected panic");
            StaticProvider::make_barrier()
        }
        fn discard_barrier(&self, _value: Handle) {
            assert_ne!(self.0, "discard", "stateful retirement injected panic");
        }
    }
    if let Ok(mode) = std::env::var(MODE) {
        let policy = PanicPolicy(mode);
        let mut map = PathMap::new();
        map.set_val_at([0, 1, 2], real(0));
        assert_eq!(retire_pathmap(map, &policy, &policy), 1);
        assert_ne!(policy.0, "dispose", "the injected disposer panic must not return");
        return;
    }
    for mode in ["make", "discard", "dispose"] {
        let output = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("rspace::guarded_pathmap::stateful_retirement::stateful_retirement_panics_fail_stop_without_raw_unwind")
            .arg("--nocapture")
            .env(MODE, mode)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output().unwrap();
        if mode != "dispose" {
            assert!(output.status.success(), "{mode}: {output:?}");
            continue;
        }
        assert_eq!(output.status.signal(), Some(6), "{mode}: {:?}", output);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("stateful retirement injected panic")
        );
    }
}

proptest! {
    #[test]
    fn stateful_provider_erases_to_static_drain_with_exact_real_values(
        entries in prop::collection::vec((prop::collection::vec(0u8..8, 0..32), any::<u16>()), 0..40),
    ) {
        let expected: BTreeMap<_, _> = entries.into_iter().collect();
        let mut map = PathMap::new();
        for (key, value) in &expected {
            map.set_val_at(key, real(u64::from(*value)));
        }
        map.create_path([255; 40]);
        let mut sibling = map.clone();
        let dynamic_dispose = Disposer::default();
        let static_dispose = Disposer::default();
        let provider = Provider::default();
        let mut dynamic_events = Observer::default();
        let mut static_events = Observer::default();
        let dynamic_count = drain_pathmap_with_provider(&mut map, &dynamic_dispose, &provider, &mut dynamic_events);
        let static_count = drain_pathmap_observed::<_, StaticProvider, _, _>(
            &mut sibling, &static_dispose, &mut static_events,
        );
        prop_assert_eq!(dynamic_count, expected.len());
        prop_assert_eq!(static_count, expected.len());
        prop_assert_eq!(dynamic_events.0, static_events.0);
        let expected_values: Vec<_> = expected.values().map(|value| real(u64::from(*value))).collect();
        prop_assert_eq!(&*dynamic_dispose.0.lock().unwrap(), &expected_values);
        prop_assert_eq!(&*static_dispose.0.lock().unwrap(), &expected_values);
        prop_assert!(map.is_empty());
        prop_assert!(sibling.is_empty());
        provider.assert_balanced();
    }
}
