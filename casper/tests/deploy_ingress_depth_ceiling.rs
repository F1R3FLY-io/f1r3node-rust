//! # The gRPC ingress depth ceiling — historical defect and current flatness gate
//!
//! ## What was wrong
//!
//! `casper/src/rust/engine/multi_parent_casper/block_admission.rs` —
//! `admit_deploy` and `admit_deploy_cosigned` — validated a deploy by parsing its
//! term and **throwing the result away**:
//!
//! ```text
//!   gRPC DeployService/doDeploy                      unauthenticated network input
//!        ▼                                           16 MiB inbound cap
//!   block_api::deploy_cosigned                       SYNCHRONOUS, inline on a
//!        ▼                                           2 MiB tokio worker
//!   dispatch ▸ admit_deploy{,_cosigned}
//!        ▼
//!   normalizer_env_from_{,cosigned_}deploy(&deploy)  O(1) in term depth
//!        ▼
//!   mk_term(&deploy.data.term, env)                  Θ(1) native stack (Stage G)
//!        │   Ok(_parsed_term)  ← BOUND AND DISCARDED
//!        ▼
//!   … the match arm ends, the Par falls out of scope …   ★ Θ(depth) derived Drop
//! ```
//!
//! ★ **`_parsed_term` was never needed.** It was bound with a leading underscore
//! and never read; the `match` is a validity check and nothing downstream consumes
//! the AST — what admission stores is the deploy's **source** (`add_deploy` /
//! `add_deploy_cosigned` persist the `Signed`/`Cosigned<DeployData>`), and the
//! block creator re-normalizes it later. The entire Θ(depth) traversal was the
//! *destructor of a value nothing reads*.
//!
//! And the failure mode was not a rejected deploy. A native-stack overflow is a
//! `SIGSEGV` on the guard page, which Rust's handler turns into
//! `fatal runtime error: stack overflow` + `abort()` — **signal 6, shell status
//! 134**. The `Err(interpreter_error) => …parsing_error(…)` arm sitting three
//! lines away in the same `match` could not see it, and no `catch_unwind` could
//! contain it. It took the node — before the deploy was stored, before consensus,
//! with no `RuntimeBudget` in scope.
//!
//! ## Reachability — the severity was position, not depth
//!
//! Ingress was the **highest** of the deploy path's three measured ceilings on a
//! 2 MiB worker, not the binding one:
//!
//! | path | ceiling (2 MiB worker, release) |
//! |---|---:|
//! | `env_get_deploy` (reduction) | **283** ← still the binding depth constraint |
//! | `plain_deploy` (reduction) | 6,831 |
//! | ingress `admit_deploy_cosigned` | **21,781** |
//!
//! It was nonetheless the only one that fired on unauthenticated network input, on
//! the receiving node, pre-consensus, pre-storage and pre-metering. A depth-21,782
//! deploy is **43,565 bytes** of source — against a 16 MiB inbound cap, i.e. 385×
//! more headroom than the attack needed — and its signature is trivially generated
//! with a fresh keypair. One gRPC `doDeploy` on port 40401 aborted the node.
//!
//! ## Measured, before the repair — **96.0 B/level**, and the ladder that fixes it
//!
//! Depth bisected at four stack sizes (release, 2026-07-27/28):
//!
//! | worker stack | max surviving source depth |
//! |---:|---:|
//! | 256 KiB | 2,667 |
//! | 512 KiB | 5,397 |
//! | 1 MiB | 10,859 |
//! | 2 MiB | **21,781** |
//!
//! Pairwise slopes **95.988 / 96.006 / 95.997 B/level**; least squares over the
//! four points gives **95.999 B/level** with an intercept of **6,106 B**
//! (5.96 KiB) at $`r^2 = 1.0000`$, the largest residual being 21 bytes — a fifth
//! of one level.
//!
//! The disassembly agrees to the byte. In this test binary's release codegen:
//!
//! ```text
//!   drop_in_place::<models::rhoapi::Par>                 push r15,r14,r13,r12,rbx
//!        │  2 call sites into ─────────┐                 no `sub rsp`   ⇒ 48 B
//!        ▼                             │
//!   drop_in_place::<rhoapi::expr::ExprInstance>          push r15,r14,r13,r12,rbx
//!        └─ 46 call sites back ────────┘                 no `sub rsp`   ⇒ 48 B
//!
//!                                        one nesting level = 48 + 48 = 96 B
//! ```
//!
//! i.e. the two are mutually recursive across the `EList.ps: Vec<Par>` edge, and
//! the cycle costs **96 B per level** — which is 95.999 to within the measurement.
//!
//! ⚠ **This CORRECTS the 84.3 B/level this file used to record.** That figure was
//! a two-point estimate through *minimum stack* at depths 256 and 4,096, whose low
//! point is dominated by the flat parse/normalize floor (~77 KiB) rather than by
//! the destructor; dividing a difference that includes a large constant by a short
//! span understates the slope. Four fixed-stack depth bisections do not have that
//! defect, and they agree with the instruction bytes. **84.3 is withdrawn in
//! favour of 96.0.**
//!
//! ## ★★ The call-shape question, settled
//!
//! This file previously reported that the *spelling* of the discard changed the
//! slope: `mk_term(..).map(drop)` measured 135.5 B/level where the literal
//! `match … Ok(_parsed_term) => …` measured 84.3, and concluded that a probe for a
//! destructor must reproduce the call shape literally.
//!
//! The mechanism is now known, and it is not the spelling. The **identical**
//! function `drop_in_place::<models::rhoapi::Par>` — same crate, same type, same
//! source — is emitted with **5 pushes and no `sub rsp`** (48 B) in this test's
//! release binary and with **7 pushes** (64 B) in the rholang gate's, and the
//! respective mutually-recursive cycles bisect to **96** and **144** B/level. That
//! is a 1.5× spread across two builds of one function, from LLVM register
//! allocation alone, with no source difference whatsoever. E89's 84.3-vs-135.5
//! pair is 1.6× and is therefore consistent with being exactly this codegen
//! variance rather than a law about how an owned value reaches its drop glue.
//!
//! ⚠ Both were also measured on *min-stack* ladders, which understate (see the
//! correction above), so the pair is not even two readings of the same quantity.
//!
//! ★ And it is **moot for the repair**: the worklist eliminates the recursive
//! cycle, so there is no per-level slope left for any spelling to modulate. The
//! subject below calls production rather than imitating it, so the question cannot
//! arise again here.
//!
//! ## The two repairs
//!
//! The first repair centralized admission's surplus term in
//! `validate_deploy_term`, which hands it to `par_children::dismantle`. The later,
//! schema-generated repair emits `Drop for Par` itself and delegates to
//! `par_children::dismantle_in_place`. Both paths therefore use one explicit heap
//! worklist and `O(1)` native stack. The generated repair is broader: ordinary
//! lexical destruction of any `Par` now has the same depth-independent property.
//!
//! **Zero observable bytes change.** The signature is over the SOURCE
//! (`DeployData::to_message`'s `term` field is the source string, and that is what
//! `Signed::create` / `Cosigned::from_signed_data` sign); storage is the source;
//! and the proposer re-normalizes from source when it builds a block. The repair
//! reorders the frees of a value no signature, hash, block, replay or stored byte
//! ever reads.
//!
//! ## Measured after the call-site repair
//!
//! Printed by [`ingress_validation_is_depth_independent`] on every run; the
//! figures below are one such run (2026-07-28).
//!
//! | | ladder `256 → 4,096` | slope | ceiling on a 2 MiB worker |
//! |---|---|---:|---:|
//! | **release, worklist** (production) | 77,824 → **77,824** B | **0.0** | **none below 262,144** |
//! | release, derived (control) | 77,824 → 401,408 B | 84.3 | 21,782 |
//! | **debug, worklist** (production) | 258,048 → **258,048** B | **0.0** | **none below 262,144** |
//! | debug, derived (control) | 258,048 → 1,908,736 B | 429.9 | 4,503 |
//!
//! The subject's minimum stack is **identical at both ends of the ladder, in both
//! profiles** — not merely within tolerance — because after the conversion the
//! only thing that sets it is the depth-independent parse/normalize floor.
//!
//! ⚠ The control's ceilings read 21,782 / 4,503 against the 21,781 / 4,504
//! bisected from production before the repair. The one-level disagreements are
//! the control's own ~6.1 KiB intercept moving by tens of bytes: it is now reached
//! through a `match teardown` in [`ingress_validate`] rather than being that
//! function's whole body. One level is 96 B in release, and the shift is in that
//! range in both directions, which is the same codegen sensitivity the section
//! above resolves. The numbers quoted for *production* are the pre-repair
//! production ones; the control's are its own.
//!
//! The table predates generated `Drop`. Its "derived" column is retained as a
//! historical measurement, not a current expectation: generated `Drop` has now
//! removed that slope as well.
//!
//! ## What is asserted now
//!
//! [`ingress_validation_is_depth_independent`] requires both the explicit
//! admission worklist and ordinary generated `Drop` to remain flat and uncapped.
//! Anti-vacuity no longer relies on a deliberately recursive control. The
//! generated-`Drop` path iteratively follows the normalized `EList` spine and
//! proves that the output has exactly the requested depth before releasing it.
//! Both paths consume the same signed deploy source and normalizer environment,
//! while the existing verdict-equivalence test proves that the admission wrapper
//! accepts and rejects exactly what `mk_term` does.
//!
//! ⚠ A raised floor is **not** admissible as evidence of conversion; that rule is
//! stated where the gate defines `CONVERTED_DEPTH` and it is the reason
//! [`ingress_depth_ceiling_has_not_got_worse`] — the original one-sided floor — is
//! kept but no longer carries the claim.
//!
//! Each probe point still runs in a child because a regression in either teardown
//! can abort on the native stack guard rather than return an error. The parent can
//! observe that process status without taking down the test runner.

use std::collections::HashMap;

use casper::rust::util::rholang::interpreter_util;
use crypto::rust::signatures::secp256k1::Secp256k1;
use crypto::rust::signatures::signatures_alg::SignaturesAlg;
use crypto::rust::signatures::signed::{Cosigned, Signed};
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::Par;
use models::rust::casper::protocol::casper_message::DeployData;
use models::rust::normalizer_env::normalizer_env_from_cosigned_deploy;

/// Rust's default stack for a spawned thread on Linux — what the worker handling
/// a gRPC deploy gets when nothing overrides it. Every depth-ceiling number here
/// is at this size.
const PRODUCTION_WORKER_STACK: usize = 2 * 1024 * 1024;

/// Stack-bisection granularity, in bytes. Chosen to match the rholang gate's, so
/// the two files' ladders are quantised the same way and can be compared.
const RESOLUTION: usize = 4096;

/// The maximum growth in minimum-stack, across the whole ladder, that still
/// counts as "no growth". Four bisection buckets over a ~3,840-step ladder is
/// under 5 bytes per step — far below any real per-level frame (the cheapest
/// measured member of this family is `Tree`'s `Drop` at 370 B/level).
const ZERO_SLOPE_TOLERANCE: usize = 4 * RESOLUTION;

/// The ladder's ends, in source nesting depth. Same span the gate drives
/// `normalize_drop` and `par_drop` over, for the same reason: both ends must
/// clear the composition's own ~77 KiB parse/normalize intercept, or a large
/// constant could read as a small slope.
const LADDER_LO: usize = 256;
const LADDER_HI: usize = 4_096;

/// Depth search bound. **12× the pre-repair release ceiling and 58× the debug
/// one**, so "no ceiling below this" is a claim about the repair rather than
/// about a search that stopped early. A converted ingress is flat, so the search
/// must be capped or it never terminates.
const SEARCH_CAP: usize = 1 << 18; // 262,144 → 524,289 bytes of source

/// Historical floor for the RELEASE not-worse tripwire, on
/// [`PRODUCTION_WORKER_STACK`].
///
/// Bisected 2026-07-27, **before** the repair: the ceiling was **21,781** (depth
/// 21,782 exited with signal 6). The constant sat ~13% below that, for the same
/// reason the rholang gate's tripwire ceilings sit ~1.5× above measured: ordinary
/// codegen drift must not flake a guard whose subject is a frame layout. It is a
/// FLOOR, never an equality.
///
/// ⚠ Since the conversion there is no ceiling at all below [`SEARCH_CAP`], so
/// this constant no longer bounds anything; it is retained as the recorded cost
/// of the defect. The live claim is
/// [`ingress_validation_is_depth_independent`]'s.
const CEILING_FLOOR_RELEASE: usize = 19_000;

/// The same, DEBUG. Bisected 2026-07-27 before the repair: **4,504** (429.9
/// B/level against release's 96.0). The depth a 2 MiB worker carried was
/// profile-dependent; the CLASS was not.
const CEILING_FLOOR_DEBUG: usize = 4_000;

// ---------------------------------------------------------------------------
// the two teardowns
// ---------------------------------------------------------------------------

/// How the probe releases the `Par` that deploy admission builds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Teardown {
    /// Admission's explicit `par_children::dismantle` path.
    Worklist,
    /// Ordinary lexical release through schema-generated `Drop for Par`.
    GeneratedDrop,
}

impl Teardown {
    fn tag(self) -> &'static str {
        match self {
            Teardown::Worklist => "worklist",
            Teardown::GeneratedDrop => "generated-drop",
        }
    }

    fn from_tag(tag: &str) -> Self {
        match tag {
            "worklist" => Teardown::Worklist,
            "generated-drop" => Teardown::GeneratedDrop,
            other => {
                panic!("INGRESS_TEARDOWN must be `worklist` or `generated-drop`, got {other:?}")
            }
        }
    }
}

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

/// Follow the normalized singleton-list spine without recursion.
///
/// This witnesses the depth at the output of normalization. It deliberately
/// borrows the term: the generated destructor remains the operation measured
/// after this function returns.
fn normalized_list_depth(root: &Par) -> usize {
    let mut cursor = root;
    let mut depth = 0usize;

    loop {
        let [expr] = cursor.exprs.as_slice() else {
            panic!(
                "VACUOUS PROBE: expected one expression at normalized depth {depth}, got {}",
                cursor.exprs.len()
            );
        };
        match expr.expr_instance.as_ref() {
            Some(ExprInstance::EListBody(list)) => {
                let [child] = list.ps.as_slice() else {
                    panic!(
                        "VACUOUS PROBE: expected a singleton list at normalized depth {depth}, \
                         got {} elements",
                        list.ps.len()
                    );
                };
                depth += 1;
                cursor = child;
            }
            Some(ExprInstance::GInt(0)) => return depth,
            other => panic!(
                "VACUOUS PROBE: normalized list ended at depth {depth} in {other:?}, not integer 0"
            ),
        }
    }
}

/// A real, signed, single-signer `Cosigned<DeployData>` carrying a depth-`depth`
/// term — the shape `DeployData::from_proto_cosigned` produces from the wire and
/// hands to `admit_deploy_cosigned`.
///
/// ⚠ The deploy is genuinely SIGNED rather than faked, because
/// `normalizer_env_from_cosigned_deploy` reads `primary().sig` and `primary().pk`
/// to build the `rho:rchain:deployId` / `rho:rchain:deployerId` unforgeables.
/// Those `Par`s are shallow and depth-independent — which is why ingress and the
/// rholang gate's `normalize_drop` subject measure the same composition — but
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
// A probe whose term failed to parse would return `Err` in O(1) stack and could
// masquerade as a successful depth run. The probe therefore asserts source depth,
// requires an `Ok` verdict, and on the generated-`Drop` path iteratively verifies
// the normalized output depth before releasing it.
// ---------------------------------------------------------------------------

/// ★ The subject: `admit_deploy_cosigned`'s term-validation prefix.
///
/// Everything in the real function after this point — the cosigner-cap check,
/// `add_deploy_cosigned`, the latency `tracing` — is `O(1)` in the term's nesting
/// depth and touches no `Par`, so this composition is the whole of the depth
/// exposure.
///
/// ★★ **The [`Teardown::Worklist`] arm CALLS production.** It does not reproduce
/// it. An earlier draft of this file reproduced the call shape literally, on the
/// grounds that the spelling of the discard changed the measured slope by 1.6× —
/// see the module docs for why that reading is now attributed to codegen variance
/// and why it is moot once the recursion is gone. Calling the real function is
/// strictly better regardless: production and its measurement cannot drift.
fn ingress_validate(teardown: Teardown, depth: usize) {
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

    match teardown {
        // ★ PRODUCTION. `validate_deploy_term` is what both `admit_deploy` and
        // `admit_deploy_cosigned` call.
        Teardown::Worklist => {
            match interpreter_util::validate_deploy_term(&cosigned.data.term, normalizer_env) {
                Err(e) => panic!(
                    "VACUOUS PROBE: ingress REJECTED the depth-{depth} deploy ({e:?}). A \
                     rejected deploy returns in O(1) stack and would be recorded as \
                     'survived' — nothing of that depth was ever built, so nothing of that \
                     depth was ever released."
                ),
                Ok(()) => {
                    // Stands in for the arm's O(1) production work, so the
                    // optimiser cannot collapse the call to nothing.
                    std::hint::black_box(depth);
                }
            }
        }
        Teardown::GeneratedDrop => {
            let term = interpreter_util::mk_term(&cosigned.data.term, normalizer_env)
                .unwrap_or_else(|e| {
                    panic!(
                        "VACUOUS PROBE: ingress REJECTED the depth-{depth} deploy ({e:?}). A \
                         rejected deploy returns in O(1) stack, so no depth-bearing term \
                         would be released."
                    )
                });
            assert_eq!(
                normalized_list_depth(&term),
                depth,
                "VACUOUS PROBE: normalization did not preserve the requested list depth"
            );
            std::hint::black_box(&term);
            // Ordinary scope exit exercises generated `Drop for Par`.
        }
    }
}

// ---------------------------------------------------------------------------
// the child
// ---------------------------------------------------------------------------

/// The child entry point. A NO-OP without its environment, so
/// `--run-ignored all` cannot fail the suite for an unrelated reason.
#[test]
#[ignore = "child process of the ingress depth probe; driven via INGRESS_DEPTH"]
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
    let teardown = Teardown::from_tag(
        &std::env::var("INGRESS_TEARDOWN").expect("INGRESS_TEARDOWN must accompany INGRESS_DEPTH"),
    );

    // ★ EXPLICIT `stack_size`. An ambient `RUST_MIN_STACK` or a future
    // harness-default change must not move the measurement. The repository has
    // no `RUST_MIN_STACK` override; the probe nevertheless pins the production
    // worker's stack so the result remains comparable and reproducible.
    std::thread::Builder::new()
        .stack_size(stack)
        .name("ingress".to_string())
        .spawn(move || ingress_validate(teardown, depth))
        .expect("deploy_ingress_depth_ceiling: failed to spawn")
        .join()
        .expect("deploy_ingress_depth_ceiling: subject panicked");
}

/// Run one probe point. `true` iff ingress completed.
fn ingress_survives(teardown: Teardown, depth: usize, stack: usize) -> bool {
    ingress_exit(teardown, depth, stack).success()
}

/// The exit status of one probe point, so a failure can be REPORTED with its
/// signature rather than only counted.
fn ingress_exit(teardown: Teardown, depth: usize, stack: usize) -> std::process::ExitStatus {
    let exe = std::env::current_exe().expect("deploy_ingress_depth_ceiling: current_exe");
    std::process::Command::new(exe)
        .args(["--ignored", "--exact", "ingress_child"])
        .env("INGRESS_TEARDOWN", teardown.tag())
        .env("INGRESS_DEPTH", depth.to_string())
        .env("INGRESS_STACK", stack.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("deploy_ingress_depth_ceiling: failed to run child")
}

/// Greatest depth ingress survives on `stack` bytes. Exponential probe, then
/// bisect. `cap` bounds the search so a converted ingress reports `None` rather
/// than searching forever.
fn max_surviving_depth(teardown: Teardown, stack: usize, cap: usize) -> Option<usize> {
    assert!(
        ingress_survives(teardown, 1, stack),
        "HARNESS FAILURE: ingress ({}) does not run at depth 1 on a {} KiB stack. That is \
         not a depth ceiling — re-run the child directly to see the error.",
        teardown.tag(),
        stack / 1024
    );
    let mut lo = 1usize;
    let mut hi = 2usize;
    while hi <= cap && ingress_survives(teardown, hi, stack) {
        lo = hi;
        hi *= 2;
    }
    if hi > cap {
        return None; // survived the whole search range
    }
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if ingress_survives(teardown, mid, stack) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(lo)
}

/// Smallest stack (to [`RESOLUTION`] granularity) on which ingress survives at
/// `depth`. Exponential probe, then bisect — the same method, and the same
/// quantisation, the rholang gate's `min_stack_for` uses.
fn min_stack_for(teardown: Teardown, depth: usize) -> usize {
    const CEILING: usize = 512 * 1024 * 1024;
    let mut hi = 16 * 1024;
    while hi <= CEILING && !ingress_survives(teardown, depth, hi) {
        hi *= 2;
    }
    assert!(
        hi <= CEILING,
        "ingress ({}) needed more than 512 MiB at depth {}",
        teardown.tag(),
        depth
    );
    let mut lo = hi / 2;
    while hi - lo > RESOLUTION {
        let mid = (lo + hi) / 2;
        if ingress_survives(teardown, depth, mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// B/level over the ladder, for reporting.
fn slope(lo_stack: usize, hi_stack: usize) -> f64 {
    (hi_stack as f64 - lo_stack as f64) / (LADDER_HI - LADDER_LO) as f64
}

// ---------------------------------------------------------------------------
// the claim
// ---------------------------------------------------------------------------

/// Deploy admission and generated `Par` destruction are depth-independent.
///
/// Both paths must have a flat minimum-stack ladder and no ceiling below
/// [`SEARCH_CAP`]. The generated-`Drop` path proves the normalized output carries
/// the requested depth before releasing it, and [`the_ingress_probe_discriminates`]
/// proves that child-process failures are observable.
#[test]
fn ingress_validation_is_depth_independent() {
    for teardown in [Teardown::Worklist, Teardown::GeneratedDrop] {
        let lo = min_stack_for(teardown, LADDER_LO);
        let hi = min_stack_for(teardown, LADDER_HI);
        println!(
            "  ingress {} ladder {LADDER_LO} -> {LADDER_HI}: {lo} -> {hi} B \
             ({:.1} B/level)",
            teardown.tag(),
            slope(lo, hi)
        );

        assert!(
            hi <= lo + ZERO_SLOPE_TOLERANCE,
            "★ INGRESS TEARDOWN `{}` IS Θ(depth) AGAIN. It needed {hi} B of stack at depth \
             {LADDER_HI} against {lo} B at depth {LADDER_LO}, a growth of {} B ({:.1} \
             B/level) past the {ZERO_SLOPE_TOLERANCE} B tolerance. Either the explicit \
             worklist stopped being reached or generated `Drop for Par` no longer detaches \
             the complete recursive schema family.",
            teardown.tag(),
            hi.saturating_sub(lo),
            slope(lo, hi)
        );

        let ceiling = max_surviving_depth(teardown, PRODUCTION_WORKER_STACK, SEARCH_CAP);
        println!(
            "  ingress {} ceiling on a {} MiB worker (cap {SEARCH_CAP}): {ceiling:?}",
            teardown.tag(),
            PRODUCTION_WORKER_STACK / (1024 * 1024)
        );
        assert!(
            ceiling.is_none(),
            "★ INGRESS TEARDOWN `{}` stops at nesting depth {ceiling:?} on a {} MiB worker, \
             where the converted path must have no ceiling below {SEARCH_CAP}. The fixture's \
             normalized output depth was checked iteratively before generated destruction.",
            teardown.tag(),
            PRODUCTION_WORKER_STACK / (1024 * 1024)
        );
    }
}

// ---------------------------------------------------------------------------
// the historical tripwire, kept
// ---------------------------------------------------------------------------

/// **Bisect the production ceiling and report it; assert only that it has not got
/// WORSE.**
///
/// The assertion is a one-sided floor — $`D_{\max} \ge D_{\text{measured}}`$ —
/// deliberately, and for the reason
/// `the_deploy_composition_is_bounded_below_by_its_destructor` gives at length in
/// the rholang gate: *a guard that must be deleted to record success is a guard
/// that discourages success.* Pinning the equality would have made the repair fail
/// this test, so the repair's author would have had to delete the only executed
/// record of what the defect cost.
///
/// ⚠ **It stayed green through the repair, exactly as designed — and that is also
/// its limitation.** Its `None` arm asserts nothing, so a ceiling that merely rose
/// to 261,000 would satisfy it too. The claim that the traversal is *converted*
/// and not merely *cheaper* is [`ingress_validation_is_depth_independent`]'s; this
/// test is retained as the record of the defect's measured cost and as a guard
/// against a regression that reintroduces a ceiling below the historical floors.
#[test]
fn ingress_depth_ceiling_has_not_got_worse() {
    let floor = if cfg!(debug_assertions) {
        CEILING_FLOOR_DEBUG
    } else {
        CEILING_FLOOR_RELEASE
    };

    match max_surviving_depth(Teardown::Worklist, PRODUCTION_WORKER_STACK, SEARCH_CAP) {
        None => println!(
            "  ingress: NO ceiling below {} on a {} MiB worker — the Θ(depth) discard is \
             converted; this test's floor of {} is the historical cost of the defect. The \
             live claim is `ingress_validation_is_depth_independent`.",
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
                ingress_exit(Teardown::Worklist, max_depth + 1, PRODUCTION_WORKER_STACK),
                floor
            );
            assert!(
                max_depth >= floor,
                "★ THE INGRESS DEPTH CEILING GOT WORSE. Deploy admission's validation prefix \
                 now stops at nesting depth {max_depth} on the {} MiB stack a spawned worker \
                 gets, against {floor} when this was bisected before the repair. It runs \
                 BEFORE the deploy is stored, BEFORE consensus, with no `RuntimeBudget` in \
                 scope, on source that arrived from the network; and a stack overflow is a \
                 SIGSEGV, so the `Err` arm in the same `match` cannot turn it into a failed \
                 deploy. It takes the node.",
                PRODUCTION_WORKER_STACK / (1024 * 1024)
            );
        }
    }
}

/// The bidirectional half of the *harness*: the probe must be able to say NO, and
/// to say YES — for **both** teardowns.
///
/// Every assertion above is of the form "survived" or "did not survive", and a
/// probe that reported success unconditionally — because it was not reading the
/// child's exit status, or because the child was skipping its body — would
/// satisfy several of them. This drives the same probe at a stack far too small
/// for even the parser and asserts it reports failure; then at a real worker
/// stack, and asserts it reports success.
///
/// ⚠ Both rungs are deliberately far from any boundary (16 KiB against 2 MiB, at
/// depth 8) so this cannot flake on codegen drift. It asserts that the harness
/// *discriminates*, not where the boundary is.
#[test]
fn the_ingress_probe_discriminates() {
    const IMPOSSIBLY_SMALL: usize = 16 * 1024;
    for teardown in [Teardown::Worklist, Teardown::GeneratedDrop] {
        assert!(
            !ingress_survives(teardown, 8, IMPOSSIBLY_SMALL),
            "THE INGRESS PROBE CANNOT GO RED. A depth-8 deploy 'survived' a {} KiB stack \
             under teardown `{}` — far below what the parser alone needs — so the probe is \
             not observing the child's exit status and every measurement in this file is \
             vacuous.",
            IMPOSSIBLY_SMALL / 1024,
            teardown.tag()
        );
        assert!(
            ingress_survives(teardown, 8, PRODUCTION_WORKER_STACK),
            "…and the same depth-8 deploy must SUCCEED on a real worker stack under \
             teardown `{}`, or the probe rejects everything and the red half above proves \
             nothing.",
            teardown.tag()
        );
    }
}
