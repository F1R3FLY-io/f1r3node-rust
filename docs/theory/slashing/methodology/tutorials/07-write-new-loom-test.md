# 07 · Write a new Loom test

## 1 · Prerequisites

- Loom in `Cargo.toml` `dev-dependencies` (already configured for
  `casper`).
- Familiarity with the existing three Loom tests
  ([`casper/tests/slashing/loom_t_9_2_*.rs`](../../../../../casper/tests/slashing/)).
- The production code path is reachable via `loom::sync` types
  under `cfg(loom)`.

## 2 · Skeleton

Create `casper/tests/slashing/loom_t_<name>.rs`:

```rust
// Loom test for <property> under concurrent threads.
//
// Models <protocol step>; explores every interleaving permitted
// by the C11 memory model.
//
// Reference: docs/theory/slashing/methodology/randomized-search/
//            04-concurrency-interleaving-loom.md

#![cfg(loom)]

use loom::sync::{Arc, Mutex};
use loom::thread;

#[test]
fn loom_t_<name>() {
    loom::model(|| {
        let shared = Arc::new(<production type wrapping loom::sync::Mutex>);

        let h_a = {
            let shared = shared.clone();
            thread::spawn(move || {
                // <thread A operation>
            })
        };

        let h_b = {
            let shared = shared.clone();
            thread::spawn(move || {
                // <thread B operation>
            })
        };

        h_a.join().unwrap();
        h_b.join().unwrap();

        // <invariant check on shared state>
        assert!(<invariant>, "violation under schedule");
    });
}
```

## 3 · Example from this repo

See [`casper/tests/slashing/loom_t_9_2_atomic_record.rs`](../../../../../casper/tests/slashing/loom_t_9_2_atomic_record.rs)
— 200 lines, the canonical Bug #2 reproducer.

## 4 · Verification step

```sh
RUSTFLAGS="--cfg loom" cargo test --release -p casper --test mod -- loom_t_<name>
```

The flag `--release` is recommended; Loom's schedule exploration
is CPU-intensive.

A failure prints the violating schedule:

```
thread 'loom_t_<name>' panicked at 'violation under schedule', src/...
note: schedule = [Thread A: lock, Thread B: lock-wait, Thread A: write, ...]
```

## 5 · Common pitfalls

- **`std::sync` instead of `loom::sync`** — the test passes
  spuriously because Loom cannot observe the operations.
- **Missing `loom::model { … }` wrapper** — the test runs once
  with one arbitrary schedule, defeating Loom's purpose.
- **Blocking system calls** — Loom's scheduler does not handle
  real I/O.
- **Too many threads** — 4 threads is the practical maximum in
  this development; the schedule space at 5+ threads is intractable.

See [`../randomized-search/04-concurrency-interleaving-loom.md §5`](../randomized-search/04-concurrency-interleaving-loom.md)
for the full pitfall catalog.
