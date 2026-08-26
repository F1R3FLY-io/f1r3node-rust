//! Phase 9 cost-regression sample-workload harness.
//!
//! `fileio_cost_spec.rs` pins the per-native cost helper VALUES
//! (source-scan + golden numeric pins).  This file complements it by
//! MEASURING realized cost consumption: run a Rholang workload that
//! exercises fs handlers and assert the total charged (read from
//! `EvaluateResult.cost.value`) matches the sum of expected charges.
//!
//! ## Why it's separate from fileio_cost_spec
//!
//! Runtime harness has different scaffolding cost (full runtime
//! build plus tempfile fixtures) than source-scan (`include_str!`
//! plus grep).  Splitting keeps `fileio_cost_spec.rs` as a
//! fast-running check-in suite (roughly 0.02s for 68 pins) and this
//! file as a verification suite for when you need to prove the wiring
//! is live.
//!
//! ## Harness pattern
//!
//! Every test follows:
//!
//! 1. `create_metered_runtime()` — in-memory rspace + fs-native URN
//!    filter disabled.
//! 2. Rholang workload issues one or more fs native calls against a
//!    tempfile fixture.  The test constructs the source with a
//!    known-in-advance directory shape (N files, specific sizes)
//!    so the expected cost is a pure function of the fixture.
//! 3. Call `runtime.evaluate(term, Cost::create(INITIAL_PHLO, ...), ...)`
//!    with a finite initial phlo budget; read consumed from
//!    `EvaluateResult.cost.value` (this is `self.c.total_cost()` at
//!    the end of `inj_attempt` — see `interpreter.rs`).
//! 4. Assert consumed matches the sum of per-handler expected costs
//!    (from `costs.rs` helpers).
//!
//! Passing `Cost::unsafe_max()` and computing
//! `INITIAL_PHLO - runtime.cost.get().value` would overflow because
//! `inj_attempt` resets the budget from `initial_phlo` and
//! `runtime.cost.get()` then reads "remaining" from that reset
//! budget (≈ i64::MAX).  Use `EvaluateResult.cost` instead.
//!
//! ## Coverage today
//!
//! - `fs_entries` per-entry supplement: proves the two-branch
//!   charge on `fs_entries` actually consumes `50 + 32 * n`
//!   phlo for a directory with n entries (leader path).  Pins the
//!   slice 9b-iv follow-up wiring at runtime layer, complementing
//!   the source-scan pin
//!   `fs_entries_charges_supplement_on_both_branches` in
//!   `fileio_cost_spec.rs`.
//!
//! Future additions land here as one function per (workload, expected
//! cost) pair.

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::{BindPattern, ListParWithRandom, Par, TaggedContinuation};
    use rholang::rust::interpreter::accounting::costs::Cost;
    use rholang::rust::interpreter::external_services::ExternalServices;
    use rholang::rust::interpreter::io::costs::{
        fs_entries_cost, fs_entries_stream_close_cost, fs_entries_stream_next_cost,
        fs_entries_stream_open_cost, fs_entries_stream_per_entry_supplement_cost, fs_stat_cost,
    };
    use rholang::rust::interpreter::matcher::r#match::Matcher;
    use rholang::rust::interpreter::rho_runtime::{create_rho_runtime, RhoRuntime, RhoRuntimeImpl};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    fn rand() -> Blake2b512Random { Blake2b512Random::create_from_bytes(&[1, 2, 45, 65]) }

    /// Initial phlo for cost-measurement runtimes.  Any finite value
    /// at or above the expected workload cost works; 1 billion units
    /// is comfortably above any reasonable single-deploy consumption.
    const INITIAL_PHLO: i64 = 1_000_000_000;

    /// Build a runtime primed for cost measurement: in-memory rspace
    /// with the fs-native URN filter disabled so tests can bind
    /// `rho:io:fs:native:1.0.0/*` directly.  Bypassing the
    /// user-facing Fs.rho / File.rho layer isolates the native cost
    /// under test from library-layer overhead.
    async fn create_metered_runtime() -> RhoRuntimeImpl {
        let mut kvm = InMemoryStoreManager::new();
        let store = kvm.r_space_stores().await.unwrap();
        let space: RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> =
            RSpace::create(store, Arc::new(Box::new(Matcher))).unwrap();
        let runtime = create_rho_runtime(
            space,
            Arc::new(std::collections::HashMap::new()),
            true,
            &mut Vec::new(),
            ExternalServices::noop(),
        )
        .await;
        runtime.disable_fs_native_urn_filter();
        runtime
    }

    /// Runtime pin for the fs_entries per-entry two-branch charge
    /// (slice 9b-iv follow-up).  Complements the source-scan pin
    /// `fs_entries_charges_supplement_on_both_branches` in
    /// `fileio_cost_spec.rs`: that pin verifies the CALL SITE exists
    /// in the handler body; this pin verifies the calls actually
    /// consume the expected budget at runtime.
    ///
    /// Fixture: a tempdir with exactly 5 known children (four regular
    /// files + one subdirectory).  `fs_entries` on that dir should
    /// return 5 rows and consume `fs_entries_cost(5) = 50 + 32*5 = 210`
    /// units, PLUS a per-entry `fs_stat` charge for each row's kind
    /// resolution (`FS_SYSCALL_CONST = 100` per stat call = 500 for
    /// 5 entries) — total expected consumption from the fs_entries
    /// call alone is `210 + 500 = 710`.
    ///
    /// The Rholang harness deploys other primitives (send, receive,
    /// match) whose costs also charge against the same budget.
    /// Rather than pinning an exact total (which would drift on any
    /// harness change), the pin asserts:
    ///
    ///   consumed >= fs_entries_cost(5).value
    ///
    /// The LOWER BOUND is load-bearing: a regression that drops the
    /// per-entry supplement (leaving only `fs_entries_cost(0) = 50`)
    /// would fail because `50 < 210`.  Rholang harness overhead
    /// (send + receive + tuple destructure + per-row wire-encode)
    /// runs ~2500-3000 units for a 5-row workload, so an upper
    /// bound of `lower_bound + 5000` is added as a sanity ceiling —
    /// wide enough to absorb harness churn but tight enough to
    /// catch wildly-wrong helper values (e.g., an accidental
    /// `FS_ENTRIES_PER_ENTRY = 3200` typo would add 15_950 units
    /// above the current supplement and easily blow past the cap).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_entries_five_children_charges_supplement_at_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        // 4 files + 1 subdir = 5 children.  fs_entries returns rows
        // sorted lex by name, so seed with names that sort
        // deterministically for reader clarity.
        for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        std::fs::create_dir(root.join("sub")).unwrap();

        let runtime = create_metered_runtime().await;

        // Rholang: fs_entries against the root.  Since the sandbox
        // needs a canonRoot + rel pair and safe_descend rejects an
        // empty rel with FSERR_BAD_ARG "empty relative path", we
        // arrange the fixture with a parent dir and pass rel="." —
        // actually safe_descend also collapses "." to RootSelf.
        // Simplest working shape: pass root's PARENT as canonRoot
        // and root's basename as rel.  Guarantees a nested descent.
        let parent = root.parent().unwrap().to_path_buf();
        let basename = root.file_name().unwrap().to_str().unwrap().to_string();

        let term = format!(
            r#"
            new fsEntries(`rho:io:fs:native:1.0.0/entries`), e in {{
              fsEntries!("{root}", "{rel}", "oracular", *e) |
              for (@_reply <- e) {{ Nil }}
            }}
            "#,
            root = parent.display(),
            rel = basename,
        );
        let result = runtime
            .evaluate(
                &term,
                Cost::create(INITIAL_PHLO, "cost-harness initial".to_string()),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();

        let c = result.cost.value;
        let entries_cost = fs_entries_cost(5).value; // 50 + 32*5 = 210
                                                     // fs_entries additionally does a per-entry stat when
                                                     // building rows (see handlers.rs::entry_stat_row); each stat
                                                     // consumes FS_SYSCALL_CONST = 100.
        let expected_stat_cost = fs_stat_cost().value * 5;
        let lower_bound = entries_cost + expected_stat_cost;

        assert!(
            c >= lower_bound,
            "fs_entries with 5 children must consume at least {lower_bound} \
             (entries_cost {entries_cost} + 5 stat calls {expected_stat_cost}); \
             got {c}.  A regression that drops the per-entry supplement \
             (leaving only fs_entries_cost(0) = 50) would fail this lower bound."
        );
        // Upper bound: harness overhead for a 5-row workload runs
        // ~2500-3000 units of Rholang primitives (send, receive,
        // match, per-row wire-encode).  5000-unit ceiling absorbs
        // harness churn but still catches wildly-wrong helper values
        // like FS_ENTRIES_PER_ENTRY = 3200 (would add 15_950 units).
        assert!(
            c < lower_bound + 5000,
            "fs_entries with 5 children consumed {c}, which is more than \
             {} above the expected {lower_bound}.  Either the harness \
             overhead ballooned or a cost helper's coefficient drifted; \
             investigate the cost helpers in `rholang/src/rust/interpreter/io/costs.rs`.",
            c - lower_bound,
        );
    }

    /// Companion boundary pin: fs_entries on an EMPTY directory
    /// consumes only the setup charge (no per-entry supplement),
    /// confirming the supplement is proportional (0 when n=0) rather
    /// than a fixed floor.  Guards against an off-by-one that would
    /// charge `50 + 32 * (n+1)` = 82 for n=0.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_entries_empty_dir_charges_setup_only_at_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let empty_sub = root.join("empty");
        std::fs::create_dir(&empty_sub).unwrap();

        let runtime = create_metered_runtime().await;

        let term = format!(
            r#"
            new fsEntries(`rho:io:fs:native:1.0.0/entries`), e in {{
              fsEntries!("{root}", "empty", "oracular", *e) |
              for (@_reply <- e) {{ Nil }}
            }}
            "#,
            root = root.display(),
        );
        let result = runtime
            .evaluate(
                &term,
                Cost::create(INITIAL_PHLO, "cost-harness initial".to_string()),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();

        let c = result.cost.value;
        let setup_only = fs_entries_cost(0).value; // 50
                                                   // Upper bound: setup_only + Rholang harness overhead ceiling.
                                                   // Empty dir consumes NO per-entry stat calls (no rows).
        assert!(
            c >= setup_only,
            "empty-dir fs_entries must consume at least the setup cost \
             {setup_only}; got {c}"
        );
        assert!(
            c < setup_only + 2000,
            "empty-dir fs_entries consumed {c}, more than {} above the \
             setup-only expectation {setup_only}.  A regression that fires \
             the per-entry supplement with n=1 (off-by-one) would push \
             consumed to at least {} — trip investigation.",
            c - setup_only,
            setup_only + 32,
        );
    }

    /// Runtime pin for the streaming-primitive per-call two-branch
    /// charge (streaming-backing slice Step 7, 2026-08-25).
    /// Companion to `fs_entries_five_children_charges_supplement_at_runtime`:
    /// same 5-child fixture, driven through the per-fd streaming
    /// primitive (`entriesStreamOpen` / `entriesStreamNext` ×6 /
    /// `entriesStreamClose`) instead of the bulk `fs_entries`.
    ///
    /// Expected native charges under oracular mode:
    ///   * `entriesStreamOpen`: `fs_entries_stream_open_cost()` = 50.
    ///   * `entriesStreamNext` (×5 yielding): each charges
    ///     `fs_entries_stream_next_cost()` = 50 up-front plus
    ///     `fs_entries_stream_per_entry_supplement_cost(1)` = 32
    ///     post-reply, for 82 units per call.
    ///   * `entriesStreamNext` (×1 EOS): `fs_entries_stream_next_cost()`
    ///     = 50, then `fs_entries_stream_per_entry_supplement_cost(0)`
    ///     = 0 (early-return in `reserve_incremental_primitive` — the
    ///     zero-cost guard that motivated the switch away from
    ///     `reserve_primitive` for the supplement, ac7bb9b6a commit
    ///     body).
    ///   * `entriesStreamClose`: `fs_entries_stream_close_cost()` =
    ///     `fs_close_cost()` = 100.
    ///   Total native: 50 + 5*82 + 50 + 100 = 610 units.
    ///
    /// The streaming variant does NOT do a per-entry `fs_stat` (unlike
    /// bulk `fs_entries`, whose row build calls `entry_stat_row` per
    /// child) — `readdir_one_entry` derives the record from the dirent
    /// directly.  So no `+ 5 * fs_stat_cost()` term here.
    ///
    /// Assertion posture matches the bulk pin: a lower bound (regression
    /// on any dropped charge fails), plus an upper ceiling of
    /// `lower_bound + 5000` (Rholang harness overhead is per-COMM so it
    /// scales with the 8 native calls here — wider than a single-native
    /// workload but the ceiling still catches wildly-wrong helper values).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fs_entries_stream_streams_five_children_charges_supplement_at_runtime() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        // Same 5-child shape as the bulk pin: 4 files + 1 subdir.
        for name in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            std::fs::write(root.join(name), b"x").unwrap();
        }
        std::fs::create_dir(root.join("sub")).unwrap();

        let runtime = create_metered_runtime().await;

        // safe_descend needs a canonRoot + non-empty rel pair, so pass
        // root's parent as canonRoot and its basename as rel — same
        // pattern as the bulk pin above.
        let parent = root.parent().unwrap().to_path_buf();
        let basename = root.file_name().unwrap().to_str().unwrap().to_string();

        // Six Next calls (5 yields + 1 EOS terminator) driven serially
        // via a nested `for` chain.  Chaining rather than a recursive
        // contract keeps the harness overhead deterministic — every
        // `next` fires against the same fd captured from the open
        // reply, so the natives run in a fixed order and the readdir
        // cursor advances one entry at a time.
        let term = format!(
            r#"
            new fsOpen(`rho:io:fs:native:1.0.0/entriesStreamOpen`),
                fsNext(`rho:io:fs:native:1.0.0/entriesStreamNext`),
                fsClose(`rho:io:fs:native:1.0.0/entriesStreamClose`),
                o in {{
              fsOpen!("{root}", "{rel}", "oracular", *o) |
              for (@[true, fd] <- o) {{
                new n1 in {{
                  fsNext!(fd, *n1) |
                  for (@_r1 <- n1) {{
                    new n2 in {{
                      fsNext!(fd, *n2) |
                      for (@_r2 <- n2) {{
                        new n3 in {{
                          fsNext!(fd, *n3) |
                          for (@_r3 <- n3) {{
                            new n4 in {{
                              fsNext!(fd, *n4) |
                              for (@_r4 <- n4) {{
                                new n5 in {{
                                  fsNext!(fd, *n5) |
                                  for (@_r5 <- n5) {{
                                    new n6 in {{
                                      fsNext!(fd, *n6) |
                                      for (@_r6 <- n6) {{
                                        new c in {{
                                          fsClose!(fd, *c) |
                                          for (@_cr <- c) {{ Nil }}
                                        }}
                                      }}
                                    }}
                                  }}
                                }}
                              }}
                            }}
                          }}
                        }}
                      }}
                    }}
                  }}
                }}
              }}
            }}
            "#,
            root = parent.display(),
            rel = basename,
        );
        let result = runtime
            .evaluate(
                &term,
                Cost::create(INITIAL_PHLO, "cost-harness initial".to_string()),
                std::collections::HashMap::new(),
                rand(),
            )
            .await
            .unwrap();

        let c = result.cost.value;
        // Sum matches the pickup-doc formula for Step 7:
        //   open + 5*next + 5*supplement(1) + 1*next + close.
        let open = fs_entries_stream_open_cost().value;
        let next5 = 5 * fs_entries_stream_next_cost().value;
        let supp = 5 * fs_entries_stream_per_entry_supplement_cost(1).value;
        let next_eos = fs_entries_stream_next_cost().value;
        let close = fs_entries_stream_close_cost().value;
        let lower_bound = open + next5 + supp + next_eos + close;

        assert!(
            c >= lower_bound,
            "streaming entries with 5 children must consume at least \
             {lower_bound} (open {open} + 5*next {next5} + 5*supplement(1) \
             {supp} + eos-next {next_eos} + close {close}); got {c}.  A \
             regression that drops the per-entry supplement (or that \
             replaces `reserve_incremental_primitive` with `reserve_primitive` \
             on the EOS branch) would fail this lower bound."
        );
        // Ceiling: 30_000 above lower_bound.  The 8 native calls here
        // (1 open + 6 next + 1 close) each drag a per-COMM Rholang
        // overhead, and the nested-`for` chain adds match/tuple work
        // per level, so total harness cost runs ~21_000 units on top
        // of the ~600-unit native contribution — an order of magnitude
        // wider than the bulk `fs_entries` pin's ceiling.  The 30_000
        // cap still catches wildly-wrong helper coefficients: an
        // accidental `FS_ENTRIES_PER_ENTRY = 3200` typo would add
        // ~15_950 units per yielded entry (5 * 3168 ≈ 15_840) and
        // easily blow past the ceiling.
        assert!(
            c < lower_bound + 30_000,
            "streaming entries with 5 children consumed {c}, which is \
             more than {} above the expected {lower_bound}.  Either the \
             harness overhead ballooned or a cost helper's coefficient \
             drifted; investigate `fs_entries_stream_*_cost` in \
             `rholang/src/rust/interpreter/io/costs.rs`.",
            c - lower_bound,
        );
    }
}
