//! # The gRPC INGRESS depth ceiling — a MEASUREMENT, not a repair
//!
//! ⚠ **Nothing here fixes anything.** This file characterises a defect that is
//! still present, so that the repair — which touches the deploy-admission path
//! and therefore deserves a design of its own — can be scoped from measured
//! numbers instead of from arithmetic. Its assertions are chosen so that they
//! stay true *after* a fix as well as before; see
//! [`ingress_depth_ceiling_has_not_got_worse`].
//!
//! ## What is being measured
//!
//! `casper/src/rust/engine/multi_parent_casper/block_admission.rs` —
//! `admit_deploy` (`:68-80`) and `admit_deploy_cosigned` (`:112-124`) — validates
//! a deploy by parsing its term and **throwing the result away**:
//!
//! ```text
//!   gRPC DeployService/doDeploy
//!        ▼
//!   MultiParentCasper::deploy → block_admission::admit_deploy{,_cosigned}
//!        ▼
//!   normalizer_env_from_{,cosigned_}deploy(&deploy)      O(1) in term depth
//!        ▼
//!   mk_term(&deploy.data.term, env)                      Θ(1) native stack (Stage G)
//!        │   = Compiler::source_to_adt_with_normalizer_env
//!        │   Ok(_parsed_term)  ← BOUND AND DISCARDED
//!        ▼
//!   … the match arm ends, the Par falls out of scope …   ★ Θ(depth) derived Drop
//! ```
//!
//! ★ **`_parsed_term` is not needed.** It is bound with a leading underscore and
//! never read; the `match` is a validity check and nothing downstream consumes the
//! AST — what admission stores is the deploy's **source** (`add_deploy` /
//! `add_deploy_cosigned` persist the `Signed`/`Cosigned<DeployData>`), and the
//! block creator re-normalizes it later. So the entire Θ(depth) traversal charged
//! here is the *destructor of a value nothing reads*.
//!
//! ## Why this instance is the most exposed member of the family
//!
//! 1. **Pre-consensus.** It fires on the spawned worker handling the gRPC call,
//!    before the deploy is written to `KeyValueDeployStorage` and long before any
//!    replica has agreed to spend anything on it.
//! 2. **Unbudgeted.** `admit_deploy_cosigned` never touches a `RuntimeBudget`.
//!    Cost accounting cannot bound this — not because the charge would be too
//!    small, but because no charge exists yet. That is the argument the audit
//!    makes for `build-normalized-term` (§14, E53), one layer further out.
//! 3. **Unauthenticated in effect.** The source arrives from the network, and a
//!    deeply nested literal is free to produce: `[`×d `0` `]`×d is `2d + 1` bytes.
//!
//! And the failure mode is not a rejected deploy. A native-stack overflow is a
//! `SIGSEGV` on the guard page, which Rust's handler turns into
//! `fatal runtime error: stack overflow` + `abort()` — **signal 6, shell status
//! 134**. The `Err(interpreter_error) => …parsing_error(…)` arm sitting three
//! lines away in the same `match` cannot see it, and no `catch_unwind` can
//! contain it. It takes the node.
//!
//! ## Measured — 2 MiB thread, direct bisection, BOTH profiles (2026-07-27)
//!
//! | | RELEASE | DEBUG |
//! |---|---:|---:|
//! | max surviving source depth | **21,781** | **4,504** |
//! | first failing depth / exit | 21,782 → signal 6 | 4,505 → signal 6 |
//! | min stack @ depth 256 | 77,824 B | 258,048 B |
//! | min stack @ depth 4,096 | 401,408 B | 1,908,736 B |
//! | derived slope | **84.3 B/level** | **429.9 B/level** |
//!
//! The DEBUG slope agrees with the `normalize_drop` and `par_drop` gate subjects'
//! independently bisected **464 B/level** to within 8%, which is the cross-check
//! that ingress and those subjects really are the same composition — the residue
//! being the deploy envelope's fixed setup rather than anything depth-dependent.
//!
//! ⚠ The RELEASE slope (84.3) sits *below* `par_drop`'s release figure (144) on
//! the same ladder. That is not a contradiction and it is not noise: the two
//! fixtures differ (`par_drop` tears down a hand-built `EList` chain; this one
//! tears down a NORMALIZED, sorted term), and 84 vs 144 B/level is well inside the
//! per-call-site frame-layout variation this family shows everywhere at `-O2`.
//! The profile ratio for THIS composition is 429.9 / 84.3 = **5.1×**, against
//! `par_drop`'s 464 / 144 = **3.2×** — per-fixture, as the audit's §5 table
//! already records for every other member.
//!
//! **A control was measured too, and NOT applied.** Routing the same probe
//! through a helper that hands the term to
//! `models::rust::rholang::par_children::dismantle` — an explicit-worklist
//! teardown — makes it **flat**: 77,824 B at both depth 256 and depth 4,096
//! (0 B/level), with no ceiling found below the bisection's own cap of 1,048,576.
//! Recorded here as evidence about the size of the available headroom. The repair
//! itself is out of scope: deploy admission is pre-metering and pre-storage, and
//! a change there warrants its own design.
//!
//! ⚠⚠ **And the control taught something about the measurement.** That helper
//! form, with its discard written as `mk_term(..).map(drop)`, measured
//! **135.5 B/level** and a ceiling of **14,520** when it was left un-repaired —
//! against 84.3 and 21,781 for the literal `match … Ok(_parsed_term) => …` shape,
//! on the same build and fixture. A 1.6× difference in the *repeating* cost, from
//! nothing but how the owned `Par` reaches its drop glue. The numbers above are
//! therefore reported for the shape `block_admission.rs` actually contains, and
//! [`ingress_validate`] reproduces that shape literally rather than refactoring
//! it.
//!
//! ## Why a child process per probe point
//!
//! The failure is an `abort()`, so it cannot be observed in-process without
//! taking every other assertion in the binary with it. The parent re-execs this
//! binary once per (depth, stack) and reads the exit status.
//!
//! ```text
//! INGRESS_DEPTH=21781 INGRESS_STACK=2097152 \
//!   cargo test --release -p casper --test deploy_ingress_depth_ceiling \
//!   -- --ignored --exact ingress_child
//! ```

use std::collections::HashMap;

use casper::rust::util::rholang::interpreter_util;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::normalizer_env::normalizer_env_from_cosigned_deploy;

/// Rust's default stack for a spawned thread on Linux — what the worker handling
/// a gRPC deploy gets when nothing overrides it. Every number here is at this
/// size.
const PRODUCTION_WORKER_STACK: usize = 2 * 1024 * 1024;

/// Floor for the RELEASE not-worse assertion, on [`PRODUCTION_WORKER_STACK`].
///
/// Bisected 2026-07-27: the ceiling is **21,781** (depth 21,782 exits with signal
/// 6). The constant is set ~13% below that, for the same reason the rholang
/// gate's tripwire ceilings sit ~1.5× above measured: ordinary codegen drift
/// must not flake a guard whose subject is a frame layout. It is a FLOOR, never
/// an equality — a repair raises the ceiling without limit and must leave this
/// green.
const CEILING_FLOOR_RELEASE: usize = 19_000;

/// The same, DEBUG. Bisected 2026-07-27: **4,504** (429.9 B/level against
/// release's 84.3 — a 5.1× profile ratio for this composition). The depth a
/// 2 MiB worker carries is profile-dependent; the CLASS is not.
const CEILING_FLOOR_DEBUG: usize = 4_000;

// ---------------------------------------------------------------------------
// the deploy — built ITERATIVELY, so the builder is never the constraint
// ---------------------------------------------------------------------------

/// `[[[…[0]…]]]` — the 577-byte reproducer's shape at `depth = 288`.
fn nested_list_source(depth: usize) -> String {
    let mut s = String::with_capacity(2 * depth + 1);
    for _ in 0..depth {
        s.push('[');
    }
    s.push('0');
    for _ in 0..depth {
        s.push(']');
    }
    s
}

/// The leading bracket run of a source — the parameter the probe claims to carry.
fn source_bracket_depth(src: &str) -> usize { src.bytes().take_while(|b| *b == b'[').count() }

/// A real, signed, single-signer `Cosigned<DeployData>` carrying a depth-`depth`
/// term — the shape `DeployData::from_proto_cosigned` produces from the wire and
/// hands to `admit_deploy_cosigned`.
///
/// ⚠ The deploy is genuinely SIGNED rather than faked, because
/// `normalizer_env_from_cosigned_deploy` reads `primary().sig` and `primary().pk`
/// to build the `rho:rchain:deployId` / `rho:rchain:deployerId` unforgeables.
/// Those `Par`s are shallow and depth-independent — which is exactly why ingress
/// and the `normalize_drop` gate subject measure the same composition — but
/// building them from a real envelope is what makes this the production entry
/// rather than a re-implementation of it.
fn cosigned_deploy_of_depth(depth: usize) -> Cosigned<DeployData> {
    let data = DeployData {
        term: nested_list_source(depth),
        time_stamp: 100,
        valid_after_block_number: 0,
        shard_id: "root".to_string(),
        expiration_timestamp: None,
    };
    let (sk, _pk) = Secp256k1.new_key_pair();
    let signed = Signed::create(data, Box::new(Secp256k1), sk)
        .expect("deploy_ingress_depth_ceiling: failed to sign the deploy");
    Cosigned::from_single_signer(signed)
        .expect("deploy_ingress_depth_ceiling: single-signer uplift must work")
}

// ---------------------------------------------------------------------------
// ⚠ ANTI-VACUITY
//
// A probe whose term failed to PARSE would return `Err` in O(1) stack, exit 0,
// and be recorded as "survived" — reporting a ceiling for a traversal that never
// ran. That is the same failure the audit records three times (§4.3, §11.3, the
// score-tree subjects). So the probe checks the source's bracket run AND asserts
// the verdict is `Ok`: ingress accepted the deploy, which means `mk_term`
// normalized it in full and something of that depth existed to be released.
// ---------------------------------------------------------------------------

/// ★ The subject: `admit_deploy_cosigned`'s term-validation prefix, verbatim.
///
/// Everything in the real function after this point — the cosigner-cap check,
/// `add_deploy_cosigned`, the latency `tracing` — is `O(1)` in the term's nesting
/// depth and touches no `Par`, so this composition is the whole of the depth
/// exposure.
///
/// ⚠⚠ **The `match` below is written LITERALLY, and that is not pedantry.** An
/// earlier draft of this probe factored the composition into a helper returning
/// `Result<(), _>` and wrote `mk_term(..).map(drop)`. It measured **135.5
/// B/level** and a ceiling of **14,520** — against **84.3 B/level** and **21,781**
/// for the shape below, on the same build, the same fixture and the same stack.
/// A 1.6× difference in the *repeating* cost, from nothing but how the owned
/// `Par` reaches its drop glue.
///
/// So the number this file reports is only meaningful for the code shape it
/// actually measures, and the shape it measures is
/// `admit_deploy_cosigned`'s: `match mk_term(…) { …, Ok(_parsed_term) => { …O(1)
/// work…; } }`, with the term released by the arm ending rather than by a `drop`
/// call this test wrote.
fn ingress_validate(depth: usize) {
    let cosigned = cosigned_deploy_of_depth(depth);
    assert_eq!(
        source_bracket_depth(&cosigned.data.term),
        depth,
        "VACUOUS PROBE: the deploy's SOURCE carries {} brackets but the subject was asked \
         for {}. A collapsed fixture runs in O(1) stack and reports a comfortable ceiling \
         for a traversal that was never given any depth.",
        source_bracket_depth(&cosigned.data.term),
        depth
    );
    let normalizer_env: HashMap<String, models::rhoapi::Par> =
        normalizer_env_from_cosigned_deploy(&cosigned);

    match interpreter_util::mk_term(&cosigned.data.term, normalizer_env) {
        Err(e) => panic!(
            "VACUOUS PROBE: ingress REJECTED the depth-{depth} deploy ({e:?}). A rejected \
             deploy returns in O(1) stack and would be recorded as 'survived' — nothing of \
             that depth was ever built, so nothing of that depth was ever released."
        ),
        // ⚠ THE ARM UNDER TEST. Production does `O(1)` work here — a cosigner-count
        // check, a store write, two `tracing` calls — and then the arm ends and
        // `_parsed_term` falls out of scope through `prost`'s derived, recursive
        // `drop_in_place`. `black_box` stands in for that work so the optimiser
        // cannot collapse the arm to nothing; the drop is left IMPLICIT, exactly as
        // production leaves it.
        Ok(_parsed_term) => {
            std::hint::black_box(depth);
        }
    }
}

// ---------------------------------------------------------------------------
// the child
// ---------------------------------------------------------------------------

/// The child entry point. A NO-OP without its environment, so
/// `--run-ignored all` cannot fail the suite for an unrelated reason.
#[test]
#[ignore = "child process of the ingress depth-ceiling probe; driven via INGRESS_DEPTH"]
fn ingress_child() {
    let Ok(depth) = std::env::var("INGRESS_DEPTH") else {
        println!("ingress_child: no INGRESS_DEPTH — not a child invocation, nothing to do");
        return;
    };
    let depth: usize = depth.parse().expect("INGRESS_DEPTH must be an integer");
    let stack: usize = std::env::var("INGRESS_STACK")
        .expect("INGRESS_STACK must accompany INGRESS_DEPTH")
        .parse()
        .expect("INGRESS_STACK must be an integer");

    // ★ EXPLICIT `stack_size`. This repository's `.cargo/config.toml` sets
    // `RUST_MIN_STACK = 8388608`, so a probe that let the default stand would be
    // measuring a 4× larger stack than a node worker has, and would report a
    // ceiling four times the production one.
    std::thread::Builder::new()
        .stack_size(stack)
        .name("ingress".to_string())
        .spawn(move || ingress_validate(depth))
        .expect("deploy_ingress_depth_ceiling: failed to spawn")
        .join()
        .expect("deploy_ingress_depth_ceiling: subject panicked");
}

/// Run one probe point. `true` iff ingress completed.
fn ingress_survives(depth: usize, stack: usize) -> bool { ingress_exit(depth, stack).success() }

/// The exit status of one probe point, so a failure can be REPORTED with its
/// signature rather than only counted.
fn ingress_exit(depth: usize, stack: usize) -> std::process::ExitStatus {
    let exe = std::env::current_exe().expect("deploy_ingress_depth_ceiling: current_exe");
    std::process::Command::new(exe)
        .args(["--ignored", "--exact", "ingress_child"])
        .env("INGRESS_DEPTH", depth.to_string())
        .env("INGRESS_STACK", stack.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("deploy_ingress_depth_ceiling: failed to run child")
}

/// Greatest depth ingress survives on `stack` bytes. Exponential probe, then
/// bisect. `cap` bounds the search so a repaired ingress reports `>= cap` rather
/// than searching forever.
fn max_surviving_depth(stack: usize, cap: usize) -> Option<usize> {
    assert!(
        ingress_survives(1, stack),
        "HARNESS FAILURE: ingress does not run at depth 1 on a {} KiB stack. That is not a \
         depth ceiling — re-run the child directly to see the error.",
        stack / 1024
    );
    let mut lo = 1usize;
    let mut hi = 2usize;
    while hi <= cap && ingress_survives(hi, stack) {
        lo = hi;
        hi *= 2;
    }
    if hi > cap {
        return None; // survived the whole search range
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if ingress_survives(mid, stack) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

// ---------------------------------------------------------------------------
// the measurement
// ---------------------------------------------------------------------------

/// ★★ **Bisect the ingress ceiling and report it; assert only that it has not got
/// WORSE.**
///
/// The assertion is a one-sided floor —
/// $`D_{\max} \ge D_{\text{measured}}`$ — deliberately, and for the reason
/// `the_deploy_composition_is_bounded_below_by_its_destructor` gives at length in
/// the rholang gate: *a guard that must be deleted to record success is a guard
/// that discourages success.* Pinning the equality would make the eventual repair
/// — which raises the ceiling without limit — fail this test, so the repair's
/// author would have to delete the only executed record of what the defect cost.
/// A floor stays green through the repair and still catches the two things worth
/// catching: a regression that lowers the ceiling, and a fixture that quietly
/// stopped carrying the depth.
///
/// The measured number is PRINTED with its bracketing evidence, so the report is
/// reproducible from the invocation in the module docs rather than from this
/// file's constants.
#[test]
fn ingress_depth_ceiling_has_not_got_worse() {
    // A repaired ingress is flat; cap the search so this test does not turn into
    // an unbounded hunt the day somebody fixes it.
    const SEARCH_CAP: usize = 1 << 18; // 262,144 — 18× the measured release ceiling

    let floor = if cfg!(debug_assertions) {
        CEILING_FLOOR_DEBUG
    } else {
        CEILING_FLOOR_RELEASE
    };

    match max_surviving_depth(PRODUCTION_WORKER_STACK, SEARCH_CAP) {
        None => println!(
            "  ingress: NO ceiling below {} on a {} MiB worker — the Θ(depth) discard has \
             evidently been repaired; this test's floor of {} is now historical",
            SEARCH_CAP,
            PRODUCTION_WORKER_STACK / (1024 * 1024),
            floor
        ),
        Some(max_depth) => {
            println!(
                "  ingress depth ceiling on a {} MiB worker: {} (depth {} exits {:?}); \
                 recorded floor {}",
                PRODUCTION_WORKER_STACK / (1024 * 1024),
                max_depth,
                max_depth + 1,
                ingress_exit(max_depth + 1, PRODUCTION_WORKER_STACK),
                floor
            );
            assert!(
                max_depth >= floor,
                "★ THE INGRESS DEPTH CEILING GOT WORSE. `admit_deploy_cosigned`'s validation \
                 prefix now stops at nesting depth {max_depth} on the {} MiB stack a spawned \
                 worker gets, against {floor} when this was bisected. That composition — \
                 `mk_term` then the derived recursive `Drop` of a term nothing reads — runs \
                 BEFORE the deploy is stored, BEFORE consensus, with no `RuntimeBudget` in \
                 scope, on source that arrived from the network; and a stack overflow is a \
                 SIGSEGV, so the `Err` arm in the same `match` cannot turn it into a failed \
                 deploy. It takes the node.",
                PRODUCTION_WORKER_STACK / (1024 * 1024)
            );
        }
    }
}

/// The bidirectional half: the probe must be able to say NO, and to say YES.
///
/// Every assertion above is of the form "survived", and a probe that reported
/// success unconditionally — because it was not reading the child's exit status,
/// or because the child was skipping its body — would satisfy all of them. This
/// drives the SAME probe at a stack far too small for even the parser, and asserts
/// it reports failure; then at a real worker stack, and asserts it reports
/// success.
///
/// ⚠ Both rungs are deliberately far from any boundary (16 KiB against 2 MiB, at
/// depth 8) so this cannot flake on codegen drift. It asserts that the harness
/// *discriminates*, not where the boundary is.
#[test]
fn the_ingress_probe_discriminates() {
    const IMPOSSIBLY_SMALL: usize = 16 * 1024;
    assert!(
        !ingress_survives(8, IMPOSSIBLY_SMALL),
        "THE INGRESS PROBE CANNOT GO RED. A depth-8 deploy 'survived' a {} KiB stack — far \
         below what the parser alone needs — so the probe is not observing the child's exit \
         status and every measurement in this file is vacuous.",
        IMPOSSIBLY_SMALL / 1024
    );
    assert!(
        ingress_survives(8, PRODUCTION_WORKER_STACK),
        "…and the same depth-8 deploy must SUCCEED on a real worker stack, or the probe \
         rejects everything and the red half above proves nothing."
    );
}
