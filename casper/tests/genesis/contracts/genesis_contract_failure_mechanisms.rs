//! ★★ **The mechanism of every genesis suite that goes red now that the suites RUN.**
//!
//! Repairing `RhoSpec::get_results` (`92a6f36c`) made sixteen suites execute for the first time.
//! Twelve pass. This file is the measurement for the ones that do not: each cell drives the
//! **same** genesis-scoped runtime and the **same** `get_results` the suites use, against a
//! synthetic one-test suite that reproduces the failure, and pins the outcome.
//!
//! ⚠ **These cells pin BEHAVIOUR, not approval.** Where a cell records that a blessed contract
//! raises, the pin exists so the defect cannot be lost and so a future fix has something to turn
//! green. It is not a statement that raising is correct.
//!
//! # ⚠ Why every probe goes through `get_results` and not through a bare `inj`
//!
//! **A Rholang program that blocks is not an error**, so `assert!(eval(code).is_ok())` is a vacuous
//! control. The instrument that fixes this — and the full rationale, measured the hard way inside
//! this file's own first version — now lives in [`super::rho_spec_probe`], because a second file
//! measures with it. [`SuiteOutcome`] reports *reached the end* separately from *did not raise*, and
//! no cell here is allowed to conclude anything from the second alone.
//!
//! # Why probes and not a bisected `.rho`
//!
//! `InterpreterError::ReduceError` carries no source position — `"Error: Multiple expressions
//! given."` names neither the term nor the file. Narrowing a 14-test fixture by editing it would
//! mutate the corpus under measurement; narrowing by *reconstruction* leaves the corpus alone and
//! produces a permanent, self-contained statement of the mechanism.

use crate::genesis::contracts::rho_spec_probe::{
    one_test_suite, only, run_suite, run_suite_with_verdicts, SuiteOutcome, PROBE_TEST,
};

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (a) `MakeMint.rho`'s former logging race stays closed
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **`MakeMintTest.rho`'s logging cell completes repeatedly after dispatch separation.**
///
/// Before the repair, the full and narrowed fixtures alternated between completion and
/// `parallel or non expression found where expression expected`. The three-argument free-pattern
/// decrement listener could consume the three-argument `setLog` send and bind a channel into the
/// amount position. R1 gives decrement a literal `"decr"` tag and a fourth argument, so the two
/// receive languages are disjoint before either body executes.
///
/// This cell retains the original narrowed-fixture method rather than substituting a hand-built
/// approximation. [`with_single_registered_test`] verifies that each source contains exactly the
/// named registered test. `test_deposit` remains the logging-disabled control; `test_log_set` is
/// repeated so the formerly intermittent interleaving cannot hide behind one favorable draw.
#[tokio::test]
async fn makemint_test_log_set_is_deterministic_after_dispatch_separation() {
    let fixture = crate::util::rholang::test_rho_loader::load_test_rho("MakeMintTest.rho")
        .expect("MakeMintTest.rho must be loadable");

    const LOG_SET: &str = "be able to set a log channel and receive notifications";
    const DEPOSIT: &str = "Deposit should work as expected";

    let control_source = with_single_registered_test(&fixture, DEPOSIT);
    let subject_source = with_single_registered_test(&fixture, LOG_SET);

    let control = run_suite(&control_source).await;
    println!("control  ({DEPOSIT}): {control:?}");
    assert!(
        control.is_completed(),
        "★ CONTROL: `{DEPOSIT}` alone must COMPLETE — it exercises the same `deposit` contract with \
         logging at its default. If it does not, the failure is not specific to the log channel. \
         Got {control:?}",
    );
    assert_eq!(
        match &control {
            SuiteOutcome::Completed { reported } => reported.clone(),
            other => panic!("checked above: {other:?}"),
        },
        only(DEPOSIT),
        "★ CONTROL: exactly the narrowed test must have reported",
    );

    // The formerly intermittent subject is repeated under identical source and genesis state.
    const ATTEMPTS: usize = 3;
    for attempt in 1..=ATTEMPTS {
        let outcome = run_suite(&subject_source).await;
        println!("subject attempt {attempt}/{ATTEMPTS} ({LOG_SET}): {outcome:?}");
        match outcome {
            SuiteOutcome::Completed { reported } => assert_eq!(
                reported,
                only(LOG_SET),
                "★ a completing attempt must report exactly the narrowed logging test; attempt \
                 {attempt}",
            ),
            other => panic!(
                "★★ `{LOG_SET}` must complete after dispatch separation; attempt {attempt} \
                 produced {other:?}",
            ),
        }
    }
}

/// Rewrite a fixture's `testSuite` registration list down to the single entry naming `keep`.
///
/// ★ The rewrite is **verified, not assumed**: the result is put through
/// [`crate::helper::rho_spec_suite_manifest::registered_test_names`] — the same extractor the
/// non-vacuity floor derives its expectations from — and the extracted set must be exactly
/// `{keep}`. A narrowing that silently kept the whole list, or dropped everything, cannot pass.
fn with_single_registered_test(source: &str, keep: &str) -> String {
    use crate::helper::rho_spec_suite_manifest::registered_test_names;

    let selector = source
        .find(r#""testSuite""#)
        .expect("a RhoSpec fixture sends \"testSuite\"");
    let open = selector
        + source[selector..]
            .find('[')
            .expect("the registration list follows the selector");

    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut close = None;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + offset);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close.expect("the registration list's bracket is balanced");

    // The `(name, *body)` entry naming `keep`, taken verbatim from the list so the body's
    // identifier does not have to be guessed.
    let list = &source[open..=close];
    let entry_start = list
        .find(&format!("(\"{keep}\""))
        .unwrap_or_else(|| panic!("the fixture must register {keep:?}; list was {list}"));
    let entry_end = entry_start
        + list[entry_start..]
            .find(')')
            .expect("a registration entry is a closed tuple");
    let entry = &list[entry_start..=entry_end];

    let narrowed = format!("{}[{entry}]{}", &source[..open], &source[close + 1..]);

    let extracted =
        registered_test_names(&narrowed).expect("the narrowed fixture must still normalize");
    assert_eq!(
        extracted,
        only(keep),
        "★ FLOOR for the narrowing: after rewriting the list the fixture must register EXACTLY \
         {keep:?}. Anything else means the rewrite missed, and the measurement below would be of \
         the wrong program.",
    );

    narrowed
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (b) `TreeHashMapTest.rho`'s `test_update_after_delete` expects `Nil + 1` to be silent
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **`Nil + 1` RAISES; it does not "yield no result".**
///
/// `TreeHashMapTest.rho:385` (`test_update_after_delete`) carries the comment
///
/// > With the same `val + 1` update body that yields no result on Nil input, get must return Nil
///
/// and that premise is false. `combine_plus`
/// (`rholang/src/rust/interpreter/reduce.rs:3492`) dispatches on a pair of `Expr`s, and its
/// operands are produced by the coercion that rejects a `Par` holding **no** expression. `Nil` is
/// exactly that `Par`, and the arm it falls into is the catch-all that reports `"Error: Multiple
/// expressions given."` — a message that names the wrong condition for an operand that is empty
/// rather than plural, which is why the suite's failure was hard to place.
///
/// ★★ **AND THE FIXTURE IS RIGHT — the contract violates the invariant the fixture names.**
/// `test_get_after_update_nil` (`TreeHashMapTest.rho:81`) has the **identical** `ret!(val + 1)`
/// update body and PASSES, on a key that was never set. The difference is in `TreeHashMap`:
///
/// * never set — the path does not exist, so `TreeHashMapUpdater`
///   (`casper/src/main/resources/Registry.rho:288`) takes its "the path doesn't exist, there's no
///   value to update" branch at `:319-321` and returns **without calling `update`**. `Nil + 1` is
///   never evaluated.
/// * after delete — `TreeHashMapDeleter` removes the key from the leaf map at
///   `Registry.rho:393` (`val.delete(suffix)`) but leaves the leaf map in place. The updater's leaf
///   test at `Registry.rho:296` is `val == 0`, which is true of a never-written leaf (an `Int`) but
///   false of *any* `Map`, so it falls through to `update!(val.get(suffix), …)` — i.e.
///   `update!(Nil, …)`. `Nil + 1` IS evaluated, and raises.
///
/// So "Update after delete behaves like update on never-set" — the fixture's own sentence — is
/// exactly the invariant the updater fails to uphold, and the raise is only the messenger.
///
/// ★★ **FIXED, and the mechanism above needed one correction to be actionable.** This cell
/// originally described the leaf as "the now-EMPTY leaf map", which reads as *emptiness* being the
/// defect and points the repair at the deleter. That is wrong, and
/// [`super::tree_hash_map_delete_restores_never_set::update_in_a_leaf_a_sibling_still_occupies_does_not_resurrect`]
/// refutes it: a leaf that a SIBLING key still occupies — so not empty at all — resurrects the
/// deleted key just the same. The question `Registry.rho:296` asks is about the leaf's *carrier*
/// where it must be about the *key*, and the repair is `if (val.contains(suffix))` inside the lock.
///
/// The raise was also not the worst of it. With an update body that can cope with `Nil`, the updater
/// wrote its answer back through `val.set(suffix, newVal)` and **the deleted key came back** — a
/// data-integrity defect in a contract `Registry.rho:486` publishes to the registry as
/// `rho:lang:treeHashMap`. That is the finding; see the sibling file for the measurement, and
/// `docs/consensus/consensus-change-register.md` CBR-033 for the consensus analysis.
///
/// This cell is unaffected by the repair and stays as it is: it measures `Nil + 1` in isolation, not
/// through `TreeHashMap`, so it remains the statement that the fixture's premise about `Nil + 1` was
/// false independently of who called it.
#[tokio::test]
async fn adding_to_nil_raises_rather_than_yielding_no_result() {
    const SETUP: &str = r#"    retCh!(Nil)"#;

    // The control: the same shape with an Int, so the refusal is attributable to the `Nil`.
    let on_int = one_test_suite(
        PROBE_TEST,
        SETUP,
        r#"    new ch in {
      ch!(41) |
      for (@val <- ch) {
        rhoSpec!("assert", (42, "==", val + 1), "41 + 1 is 42", *ackCh)
      }
    }"#,
    );

    let on_nil = one_test_suite(
        PROBE_TEST,
        SETUP,
        r#"    new ch in {
      ch!(Nil) |
      for (@val <- ch) {
        rhoSpec!("assert", (Nil, "==", val + 1), "Nil + 1 is unreachable", *ackCh)
      }
    }"#,
    );

    let int_outcome = run_suite(&on_int).await;
    assert!(
        int_outcome.is_completed(),
        "★ CONTROL: `41 + 1` must complete and assert; got {int_outcome:?}",
    );
    assert_eq!(
        match &int_outcome {
            SuiteOutcome::Completed { reported } => reported.clone(),
            other => panic!("checked above: {other:?}"),
        },
        only(PROBE_TEST),
        "★ CONTROL: the assertion must be recorded under the registered name",
    );

    let nil_outcome = run_suite(&on_nil).await;
    let raise = nil_outcome.raise_text().unwrap_or_else(|| {
        panic!(
            "★★ `Nil + 1` must RAISE — `TreeHashMapTest.rho:385`'s premise is that it is silent. \
             Got {nil_outcome:?}"
        )
    });
    assert!(
        raise.contains("Multiple expressions given"),
        "★ `Nil + 1` reports the catch-all arm of the operand coercion, which spells an EMPTY \
         operand as a plural one. Got {raise}",
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (a) `PoSTest.rho`'s `closeBlock` caller went stale when `PoS.rhox` grew two parameters
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **An ARITY MISMATCH: `PoSTest.rho:704` sends 3 arguments to a 5-argument contract.**
///
/// ```text
/// PoSTest.rho:704   @PoS!("closeBlock", sysAuthToken, *ackCh0)
/// PoS.rhox:1093     contract PoS(@"closeBlock", @sysAuthToken, mintList, feeConvertList, ackCh)
/// ```
///
/// A Rholang send whose arity does not match any contract for that channel never matches: it rests
/// as data, forever, with no error. `PoSTest.rho`'s local `contract closeBlock(ackCh)` therefore
/// never answers, so every one of its callers — lines 120, 227, 289, 297, 330, 448, 467, 532, 546,
/// 679 — blocks. `pos_spec` reports the first suite's assertions and then goes quiet, which the
/// non-vacuity floor catches as `has_finished == false`, "registered 14 test(s) … reported 1".
///
/// ★ HISTORY, which fixes the classification. `mintList` entered the signature at `879e75c5`
/// ("Stage B — validator phlogiston mint at epoch/bond + Σ⟦v⟧ dual-write seam") and
/// `feeConvertList` with Stage D; `git log -- casper/src/test/resources/PoSTest.rho` shows one
/// commit, `c36613aa`, the original workspace extraction. **The caller has never been updated**,
/// and nothing noticed because `pos_spec` has never run. This is a real defect latent for the whole
/// life of the harness, not a corpus expectation that was never true: the two-argument call was
/// correct until Stage B changed the contract under it.
///
/// ★ REPAIRED at `PoSTest.rho:702-716`, and the effect measured: `pos_spec` went from reporting
/// **1 of 14** registered tests to **4 of 14** — `PoS is created with empty rewards`, `closeBlock
/// finishes successfully`, `bonding success`, `withdraw succeeds` — and now blocks at `validator is
/// paid after withdraw`. ⚠ That is a SECOND latent defect behind the first, in the same suite, and
/// it is reported rather than fixed: the non-vacuity floor keeps `pos_spec` red until it is, which
/// is the correct state for a suite whose remaining ten tests have still never been checked.
#[tokio::test]
async fn pos_close_block_arity_mismatch_is_why_pos_test_blocks() {
    // Resolve PoS and a system auth token, the way `PoSTest.rho:33-44` does.
    const SETUP: &str = r#"    new PoSCh, makeSysAuthToken(`sys:test:authToken:make`), tokenCh in {
      rl!(`rho:system:pos`, *PoSCh) |
      makeSysAuthToken!(*tokenCh) |
      for (@(_, PoS) <- PoSCh & @token <- tokenCh) {
        retCh!((PoS, token))
      }
    }"#;

    // The fixture's call: selector + token + ack. Three arguments.
    const STALE_ARITY: &str = r#"    match *setupResult {
      (PoS, token) => {
        new ackCh0, setBlockData(`rho:test:block:data:set`), blockDataSet in {
          setBlockData!("blockNumber", 1, *blockDataSet) |
          for (_ <- blockDataSet) {
            @PoS!("closeBlock", token, *ackCh0) |
            for (@ack <- ackCh0) {
              rhoSpec!("assert", true, "closeBlock answered", *ackCh)
            }
          }
        }
      }
    }"#;

    // The contract's call: selector + token + mintList + feeConvertList + ack. Five arguments.
    const CURRENT_ARITY: &str = r#"    match *setupResult {
      (PoS, token) => {
        new ackCh0, mintListCh, feeConvertListCh,
            setBlockData(`rho:test:block:data:set`), blockDataSet in {
          setBlockData!("blockNumber", 1, *blockDataSet) |
          for (_ <- blockDataSet) {
            @PoS!("closeBlock", token, *mintListCh, *feeConvertListCh, *ackCh0) |
            for (@ack <- ackCh0) {
              rhoSpec!("assert", true, "closeBlock answered", *ackCh)
            }
          }
        }
      }
    }"#;

    let stale = run_suite(&one_test_suite(PROBE_TEST, SETUP, STALE_ARITY)).await;
    let current = run_suite(&one_test_suite(PROBE_TEST, SETUP, CURRENT_ARITY)).await;

    println!("stale arity   (3 args, as PoSTest.rho:704 sends): {stale:?}");
    println!("current arity (5 args, as PoS.rhox:1093 accepts): {current:?}");

    // ★★ THE CONTROL THAT MAKES IT A MEASUREMENT: the SAME program with two more channels
    // completes. Without this rung, "the three-argument call blocks" is consistent with
    // `closeBlock` being broken for every caller.
    assert!(
        current.is_completed(),
        "★★ `closeBlock` called with the arity `PoS.rhox:1093` declares must COMPLETE. If it does \
         not, the defect is in `closeBlock` itself and not only in its stale caller, and the \
         repair to `PoSTest.rho` will not be sufficient. Got {current:?}",
    );

    assert!(
        !stale.is_completed(),
        "★★ the three-argument call must NOT complete — it is what `PoSTest.rho:704` sends and it \
         is why `pos_spec` blocks. If it now completes, `PoS.rhox` has regained a three-argument \
         `closeBlock` and this cell's analysis needs redoing. Got {stale:?}",
    );
    assert!(
        stale.raise_text().is_none(),
        "★ an unmatched send RESTS; it does not raise. A raise here would be a different \
         mechanism from the block the floor reported for `pos_spec`. Got {stale:?}",
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (b) `PoSTest.rho:818` — a SECOND arity mismatch, in the same file, that nobody had looked for
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **`PoSTest.rho:818` sent 5 arguments to the 6-argument `PoS!"slash"`.**
///
/// ```text
/// PoSTest.rho:818  @PoS!("slash", userDeployerId, hash, sysAuthToken, *slashCh)
/// PoS.rhox:774     contract PoS(@"slash", @deployerId, @blockHash, @sysAuthToken, slashedPk, returnCh)
/// ```
///
/// Same shape and same cause as `closeBlock` above — a caller that went stale when the contract grew
/// a parameter — and found by asking "what ELSE in this file has this shape?" rather than by a
/// failure report, because the test it disables is registered fourteenth and has never been reached.
///
/// # ★★ WHAT `slashedPk` IS, which is where the obvious repair goes wrong
///
/// It is an **output channel**, not the offender's public key. The tempting fix is
/// `…, sysAuthToken, slashedValidator, *slashCh)` — `slashedValidator` is bound three lines up at
/// `PoSTest.rho:807` and is exactly the pk being slashed. It is the WRONG value, and the contract
/// body says so:
///
/// - `PoS.rhox:790` resolves the offender ITSELF, from the evidence:
///   `toBeSlashed!(invalidBlocks.get(blockHash))`. The caller does not supply it.
/// - `PoS.rhox:856` then **sends on** the parameter: `slashedPk!(validator)`. It is a sink.
/// - `slash_deploy.rs:177,182` is production's own call, and it passes a channel:
///   ``slashedPk(`sys:casper:slashedPk`)`` … `@PoS!("slash", *deployerId, …, *slashedPk, *return)`,
///   where the env value is [`SlashDeploy::slashed_pk_channel`] — an unforgeable `GPrivate` derived
///   from a fixed split of the slash-deploy RNG. `SlashDeploy::post_eval` reads the published pk back
///   out of band with `get_data_par` and zeros `Σ⟦offender⟧`.
///
/// Passing `slashedValidator` would not block — Rholang lets you send on any quoted name, so
/// `slashedPk!(validator)` would deposit a datum on `@<pk bytes>` — which is exactly why it is
/// dangerous: the test would go green while exercising a channel production never uses. The repair
/// passes a fresh `new` channel, and deliberately does **not** gate `ackCh` on reading it, because
/// production does not: the idempotent branch (`PoS.rhox:812-813`) publishes nothing at all, so a
/// `for` on that channel would hang a no-op slash.
///
/// # What this cell measures
///
/// The two arities, against the live blessed `PoS.rhox`, with the 6-argument form as the control that
/// makes "the 5-argument form blocks" attributable to arity rather than to `slash` being broken.
#[tokio::test]
async fn pos_slash_arity_mismatch_is_why_the_slashing_test_can_never_report() {
    // ⚠ The setup MUST seed `invalidBlocks`, and that was learned the hard way: the first version of
    // this probe passed an arbitrary `"00".hexToBytes()` block hash with nothing seeded, and the
    // 6-argument CONTROL BLOCKED TOO. Which is exactly what a control is for — had the cell only
    // asserted "the 5-argument form blocks", it would have passed while proving nothing.
    // `PoS.rhox:782` reads `rho:casper:invalidBlocks` before it can answer, and the existing
    // end-to-end (`runtime_manager_test.rs::slash_zeros_supply_is_play_replay_deterministic`,
    // step 2) seeds it via `set_invalid_blocks` for exactly this reason. The RhoSpec runtime's
    // equivalent is `rho:test:casper:invalidBlocks:set`, registered at `rho_spec.rs:314` and used by
    // `PoSTest.rho:783,813` — so this setup mirrors the fixture rather than inventing a shortcut.
    const SETUP: &str = r#"    new PoSCh, makeSysAuthToken(`sys:test:authToken:make`), tokenCh,
        makeDeployerId(`rho:test:deployerId:make`), deployerIdCh,
        setInvalidBlocks(`rho:test:casper:invalidBlocks:set`), invalidBlocksSetCh,
        computeHash(`rho:crypto:keccak256Hash`), hashCh, activeValidatorsCh in {
      rl!(`rho:system:pos`, *PoSCh) |
      makeSysAuthToken!(*tokenCh) |
      makeDeployerId!("deployerId", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".hexToBytes(), *deployerIdCh) |
      for (@(_, PoS) <- PoSCh & @token <- tokenCh & @deployerId <- deployerIdCh) {
        @PoS!("getActiveValidators", *activeValidatorsCh) |
        for (@activeValidators <- activeValidatorsCh) {
          match activeValidators.toList().nth(3) {
            offender => {
              computeHash!(offender, *hashCh) |
              for (@hash <- hashCh) {
                setInvalidBlocks!({hash : offender}, *invalidBlocksSetCh) |
                for (_ <- invalidBlocksSetCh) {
                  retCh!((PoS, token, deployerId, hash))
                }
              }
            }
          }
        }
      }
    }"#;

    // The fixture's call before the repair: selector + deployerId + hash + token + ack. FIVE.
    const STALE_ARITY: &str = r#"    match *setupResult {
      (PoS, token, deployerId, hash) => {
        new slashCh in {
          @PoS!("slash", deployerId, hash, token, *slashCh) |
          for (@_ <- slashCh) {
            rhoSpec!("assert", true, "slash answered", *ackCh)
          }
        }
      }
    }"#;

    // The contract's call: the same plus the `slashedPk` SINK. SIX.
    const CURRENT_ARITY: &str = r#"    match *setupResult {
      (PoS, token, deployerId, hash) => {
        new slashCh, slashedPkCh in {
          @PoS!("slash", deployerId, hash, token, *slashedPkCh, *slashCh) |
          for (@_ <- slashCh) {
            rhoSpec!("assert", true, "slash answered", *ackCh)
          }
        }
      }
    }"#;

    // ★ And the WRONG repair, at the value it produces: `slashedValidator` in the sink position.
    // It does not block — Rholang lets you send on any quoted name — which is precisely why it is
    // the dangerous fix. This rung exists so "pass a channel" is a measured requirement rather than
    // a stylistic preference: the contract answers either way, and only the channel form puts the
    // offender pk anywhere a reader can find it.
    const OFFENDER_AS_SINK: &str = r#"    match *setupResult {
      (PoS, token, deployerId, hash) => {
        new slashCh, activeValidatorsCh in {
          @PoS!("getActiveValidators", *activeValidatorsCh) |
          for (@activeValidators <- activeValidatorsCh) {
            match activeValidators.toList().nth(3) {
              offender => {
                @PoS!("slash", deployerId, hash, token, offender, *slashCh) |
                for (@_ <- slashCh) {
                  rhoSpec!("assert", true, "slash answered anyway", *ackCh)
                }
              }
            }
          }
        }
      }
    }"#;

    let stale = run_suite(&one_test_suite(PROBE_TEST, SETUP, STALE_ARITY)).await;
    let current = run_suite(&one_test_suite(PROBE_TEST, SETUP, CURRENT_ARITY)).await;
    let offender_as_sink = run_suite(&one_test_suite(PROBE_TEST, SETUP, OFFENDER_AS_SINK)).await;

    println!("stale arity   (5 args, as PoSTest.rho:818 sent):     {stale:?}");
    println!("current arity (6 args, as PoS.rhox:774 accepts):     {current:?}");
    println!("6 args, offender pk in the SINK position:            {offender_as_sink:?}");

    // ★★ THE CONTROL. Without it, "the 5-argument call blocks" is equally consistent with `slash`
    // being unreachable for every caller — and as recorded above, the first form of this cell had
    // exactly that hole.
    assert!(
        current.is_completed(),
        "★★ `slash` called with the arity `PoS.rhox:774` declares must COMPLETE. If it does not, the \
         defect is in `slash` itself and not only in its stale caller, and repairing `PoSTest.rho` \
         will not be sufficient. Got {current:?}",
    );

    assert!(
        !stale.is_completed(),
        "★★ the five-argument call must NOT complete — it is what `PoSTest.rho:818` sent. If it now \
         completes, `PoS.rhox` has regained a five-argument `slash` and this cell's analysis needs \
         redoing. Got {stale:?}",
    );
    assert!(
        stale.raise_text().is_none(),
        "★ an unmatched send RESTS; it does not raise. A raise would be a different mechanism. \
         Got {stale:?}",
    );

    // ★★ THE POINT ABOUT `slashedPk`: the WRONG repair is LIVE, not dead.
    assert!(
        offender_as_sink.is_completed(),
        "★★ passing the offender pk where `slashedPk` belongs must still COMPLETE — `slashedPk` is \
         bound as a NAME, and `slashedPk!(validator)` (`PoS.rhox:856`) will happily deposit a datum \
         on the quoted pk `@<pk bytes>`. That is why \"it is in scope three lines up, so use it\" is \
         a trap: the suite goes green while publishing the offender somewhere no reader looks, and \
         `SlashDeploy::post_eval`'s `get_data_par(&slashed_pk_channel())` would find nothing. If \
         this ever BLOCKS instead, the argument for the channel form becomes a liveness argument \
         rather than a correctness one and the comment at `PoSTest.rho:818` should say so. \
         Got {offender_as_sink:?}",
    );
}

/// ★★ **REFUTED: repairing `PoSTest.rho:818` does NOT move `pos_spec` past 4-of-14 — and it cannot.**
///
/// The expectation carried into this work item was that fixing the `slash` arity would let
/// `pos_spec` report more tests, the way the `closeBlock` repair took it from 1-of-14 to 4-of-14.
/// Measured, it does not: `pos_spec` reports the same four tests before and after. That is not a
/// failed repair — it is a property of the driver.
///
/// `RhoSpecContract.rho:147` runs the registered list through `@ListOps!("foreach", …)`, and
/// `ListOps.rho:265-271` is **strictly sequential** by construction, with the comment saying why:
///
/// ```text
/// // Need return flag from `proc` in order to guarantee execution order
/// new isDone in { proc!(head, *isDone) | for(_ <- isDone){ return!(Nil) } }
/// ```
///
/// Each element's continuation is gated on the previous one's `isDone`, so **one blocked test body
/// halts the entire fold**. `PoSTest.rho`'s list puts `validator is paid after withdraw` fifth
/// (`:55`) and `Slashing transfers funds appropriately` **fourteenth** (`:65`). With entry 5 blocked,
/// entries 6..14 are unreachable regardless of their own correctness.
///
/// ⇒ Two consequences worth stating plainly:
///
/// 1. The `:818` repair is verified by [`pos_slash_arity_mismatch_is_why_the_slashing_test_can_never_report`]
///    and by nothing in `pos_spec`. A suite-level measurement was the wrong instrument here.
/// 2. `pos_spec`'s "reported 4 of 14" is a **lower bound on the number of defects**, not a count.
///    Nine further test bodies have never executed, so any of them may hide a defect of its own —
///    exactly how `:818` was hiding behind entry 5.
///
/// This cell pins the driver property, so the reasoning above cannot silently stop being true.
#[tokio::test]
async fn rho_spec_runs_registered_tests_sequentially_so_one_block_hides_all_later_tests() {
    // Two registered tests: the first blocks on a channel nobody writes, the second would report
    // instantly. If the driver were parallel, the second would report anyway.
    const BLOCKER_FIRST: &str = r#"
new rl(`rho:registry:lookup`), RhoSpecCh, setup, blocked, reachable in {
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {
    @RhoSpec!("testSuite", *setup, [("blocked", *blocked), ("reachable", *reachable)])
  } |
  contract setup(_, retCh) = { retCh!(Nil) } |
  contract blocked(rhoSpec, _, ackCh) = {
    new never in { for (_ <- never) { rhoSpec!("assert", true, "unreachable", *ackCh) } }
  } |
  contract reachable(rhoSpec, _, ackCh) = {
    rhoSpec!("assert", true, "this one needs nothing", *ackCh)
  }
}
"#;

    // The same two, order swapped. The one that CAN report is now first.
    const BLOCKER_SECOND: &str = r#"
new rl(`rho:registry:lookup`), RhoSpecCh, setup, blocked, reachable in {
  rl!(`rho:id:zphjgsfy13h1k85isc8rtwtgt3t9zzt5pjd5ihykfmyapfc4wt3x5h`, *RhoSpecCh) |
  for(@(_, RhoSpec) <- RhoSpecCh) {
    @RhoSpec!("testSuite", *setup, [("reachable", *reachable), ("blocked", *blocked)])
  } |
  contract setup(_, retCh) = { retCh!(Nil) } |
  contract blocked(rhoSpec, _, ackCh) = {
    new never in { for (_ <- never) { rhoSpec!("assert", true, "unreachable", *ackCh) } }
  } |
  contract reachable(rhoSpec, _, ackCh) = {
    rhoSpec!("assert", true, "this one needs nothing", *ackCh)
  }
}
"#;

    let blocker_first = run_suite(BLOCKER_FIRST).await;
    let blocker_second = run_suite(BLOCKER_SECOND).await;

    println!("blocked registered FIRST:  {blocker_first:?}");
    println!("blocked registered SECOND: {blocker_second:?}");

    // Neither can complete — one test never answers either way. What differs is what got REPORTED.
    let reported_when_first = match &blocker_first {
        SuiteOutcome::Blocked { reported } => reported.clone(),
        other => panic!(
            "★ a suite with a permanently blocked test must be `Blocked`, not {other:?} — if it \
             completes, `testSuiteCompleted` no longer means what this whole file assumes."
        ),
    };
    let reported_when_second = match &blocker_second {
        SuiteOutcome::Blocked { reported } => reported.clone(),
        other => panic!("★ same, with the order swapped: expected `Blocked`, got {other:?}"),
    };

    assert!(
        reported_when_first.is_empty(),
        "★★ `ListOps.foreach` must be SEQUENTIAL: with the blocked test registered FIRST, the \
         reachable test that follows it must NOT report. It reported {reported_when_first:?}. If it \
         does report, the driver has become concurrent, and then `pos_spec`'s \"reported 4 of 14\" \
         means ten INDEPENDENTLY broken tests rather than one block hiding nine unrun ones — a \
         different conclusion, and the analysis in this cell would need redoing.",
    );

    assert!(
        reported_when_second.contains("reachable"),
        "★★ THE CONTROL: the same reachable test, registered BEFORE the blocker, MUST report — \
         otherwise it is simply a broken test and the ordering above shows nothing. Reported \
         {reported_when_second:?}",
    );
}

// ════════════════════════════════════════════════════════════════════════════════════════════════
// (c) `EitherTest.rho` — a registration typo hiding an arity mismatch hiding a stale expectation
// ════════════════════════════════════════════════════════════════════════════════════════════════

/// ★★ **`fromNillableError <-` takes NO default, and `EitherTest.rho` disagreed in three places.**
///
/// Three coupled defects, each hidden by the one before it:
///
/// | | site | defect |
/// |---|---|---|
/// | (a) | `EitherTest.rho:35` | registered `*test_from_nillable_arrow` for the `fromNillableError` row ⇒ row 1's contract ran TWICE and `test_from_nillable_error_arrow` (`:104`) NEVER ran |
/// | (b) | `EitherTest.rho:107-108` | sent **4** data to `Either.rho:68`'s **3** patterns ⇒ could never match |
/// | (c) | `EitherTest.rho:112` | expected `(false, "If not Nil")` where the contract returns `(false, error)` = `(false, "Not Nil")` |
///
/// # ⚠ Which side was wrong — established, not assumed
///
/// The **caller**, on both counts. `fromNillableError` uses its input AS the Left; it has no default
/// to fall back to, so it needs no fourth datum:
///
/// ```text
/// Either.rho:75  contract Either(@"fromNillableError", @error, return) = {
/// Either.rho:76    match error { Nil => return!((true, Nil))
/// Either.rho:78                  _   => return!((false, error)) } }
/// ```
///
/// Its siblings differ precisely because they DO need one: `fromNillable` (`:21`), `fromBoolean`
/// (`:36`) and `fromSingletonList` (`:51`) each take `(selector, ch, @default, return)`, because a
/// `Nil`/`false`/empty input has to be replaced by something. `"If not Nil"` in the test is a
/// `fromNillable`-shaped default pasted onto a contract that has no such parameter — which is also
/// why (c) exists: `:112` expected the default back.
///
/// ⇒ **NOT a blessed-contract change.** `Either.rho` is untouched, so genesis does not move and no
/// hash pin is disturbed. Corroboration: `Either.rho` is single-commit here (`c36613aa`, the
/// workspace extraction) and upstream `f1r3node` carries byte-identical text for all three defects,
/// so this is an inherited bug being fixed, not a local divergence.
///
/// # ⚠ Why both were landed in ONE commit
///
/// Fixing (a) alone makes `either_spec` **HANG** instead of pass: the newly-reached contract would
/// send 4 data at a 3-pattern receive, `ch1ret`/`ch2ret` would never receive, and the `"== <-"`
/// assertions would block forever. Fixing (b) alone changes nothing observable, because (a) means
/// the code is dead. There is no ordering of two commits in which neither intermediate state is a
/// regression, so they are one commit.
///
/// # The before/after that shows (a) was real
///
/// | clue | at HEAD | after |
/// |---|---|---|
/// | `"Uses Nil when Nil"` (only `test_from_nillable_error_arrow` asserts it) | **0** | 2 |
/// | `"Uses default value when Nil"` (only `test_from_nillable_arrow` asserts it) | **4** — ran twice | 2 |
#[tokio::test]
async fn either_from_nillable_error_takes_no_default_value() {
    const SETUP: &str = r#"    new EitherCh in {
      rl!(`rho:lang:either`, *EitherCh) |
      for (@(_, Either) <- EitherCh) { retCh!(Either) }
    }"#;

    // What `EitherTest.rho:107-108` sent: selector + errorCh + a DEFAULT + return. Four.
    const STALE_ARITY: &str = r#"    match *setupResult {
      Either => {
        new ch1, ch1ret in {
          ch1!("Not Nil") |
          @Either!("fromNillableError <-", *ch1, "If not Nil", *ch1ret) |
          for (@r <- ch1ret) {
            rhoSpec!("assert", true, "fromNillableError answered", *ackCh)
          }
        }
      }
    }"#;

    // What `Either.rho:68` accepts: selector + errorCh + return. Three.
    const CURRENT_ARITY: &str = r#"    match *setupResult {
      Either => {
        new ch1, ch1ret in {
          ch1!("Not Nil") |
          @Either!("fromNillableError <-", *ch1, *ch1ret) |
          for (@r <- ch1ret) {
            rhoSpec!("assert", true, "fromNillableError answered", *ackCh)
          }
        }
      }
    }"#;

    // ★ (c): the VALUE, not just the liveness. The error itself is the Left payload.
    const VALUE_AS_CONTRACT_RETURNS: &str = r#"    match *setupResult {
      Either => {
        new ch1, ch1ret in {
          ch1!("Not Nil") |
          @Either!("fromNillableError <-", *ch1, *ch1ret) |
          rhoSpec!("assert", ((false, "Not Nil"), "== <-", *ch1ret),
                   "the ERROR is the Left payload", *ackCh)
        }
      }
    }"#;

    // ★ …and the stale expectation, at the value it refuses. `"If not Nil"` was never returnable.
    const VALUE_AS_TEST_EXPECTED: &str = r#"    match *setupResult {
      Either => {
        new ch1, ch1ret in {
          ch1!("Not Nil") |
          @Either!("fromNillableError <-", *ch1, *ch1ret) |
          rhoSpec!("assert", ((false, "If not Nil"), "== <-", *ch1ret),
                   "the DEFAULT is the Left payload", *ackCh)
        }
      }
    }"#;

    let stale = run_suite(&one_test_suite(PROBE_TEST, SETUP, STALE_ARITY)).await;
    let current = run_suite(&one_test_suite(PROBE_TEST, SETUP, CURRENT_ARITY)).await;

    println!("stale arity   (4 args, as EitherTest.rho:107 sent):  {stale:?}");
    println!("current arity (3 args, as Either.rho:68 accepts):    {current:?}");

    assert!(
        current.is_completed(),
        "★★ THE CONTROL: `fromNillableError <-` called with the arity `Either.rho:68` declares must \
         COMPLETE. If it does not, the defect is in `Either.rho` and the repair to `EitherTest.rho` \
         is not sufficient. Got {current:?}",
    );
    assert!(
        !stale.is_completed(),
        "★★ the four-argument call must NOT complete — it is what `EitherTest.rho:107-108` sent, and \
         it is why fixing the registration typo ALONE turns `either_spec` from a pass into a HANG. \
         If it now completes, `Either.rho` has grown a fourth parameter and this cell needs redoing. \
         Got {stale:?}",
    );
    assert!(
        stale.raise_text().is_none(),
        "★ an unmatched send RESTS; it does not raise. Got {stale:?}",
    );

    // ── (c): the payload, as a RED at the value it refuses plus its green twin. ──────────────────
    let (as_contract, contract_verdict) = run_suite_with_verdicts(&one_test_suite(
        PROBE_TEST,
        SETUP,
        VALUE_AS_CONTRACT_RETURNS,
    ))
    .await;
    let (as_test_expected, stale_verdict) =
        run_suite_with_verdicts(&one_test_suite(PROBE_TEST, SETUP, VALUE_AS_TEST_EXPECTED)).await;

    println!(
        "value (false, \"Not Nil\")    — the ERROR:   {as_contract:?} verdict={contract_verdict:?}"
    );
    println!("value (false, \"If not Nil\") — the DEFAULT: {as_test_expected:?} verdict={stale_verdict:?}");

    assert_eq!(
        as_contract.reported_set(),
        Some(only(PROBE_TEST)),
        "★ the value probe must reach its assertion. Got {as_contract:?}",
    );
    assert_eq!(
        contract_verdict,
        Some(true),
        "★★ `fromNillableError` on a non-Nil input MUST return `(false, <the input>)` — \
         `Either.rho:78` is literally `return!((false, error))`. Got {contract_verdict:?} from \
         {as_contract:?}",
    );

    // ★★ THE RED, at the value the corpus used to expect. Both probes are byte-identical apart from
    // the expected payload, so the differing verdict is attributable to that payload alone.
    assert!(
        as_test_expected.is_completed(),
        "★ the stale-expectation probe must still COMPLETE — the send is well-formed and only the \
         expected value differs, so this has to be an assertion FAILURE rather than a block. A block \
         would mean the two probes differ in more than the expected value, and the comparison would \
         prove nothing. Got {as_test_expected:?}",
    );
    assert_eq!(
        stale_verdict,
        Some(false),
        "★★ `(false, \"If not Nil\")` — the default-shaped expectation `EitherTest.rho:112` used to \
         carry — MUST be refused. `fromNillableError` has no default parameter to echo back \
         (`Either.rho:68,75`); `\"If not Nil\"` was never a value it could return. `Some(true)` here \
         would mean `Either.rho` grew a default and the corpus was right all along; `None` would \
         mean the assertion never ran and this cell is vacuous. Got {stale_verdict:?} from \
         {as_test_expected:?}",
    );
}

/// ★★ **`rho:test:deployerId:make` takes THREE arguments, and getting that wrong looks EXACTLY like
/// the defect under investigation.**
///
/// While building [`pos_slash_arity_mismatch_is_why_the_slashing_test_can_never_report`], its
/// 6-argument CONTROL blocked — which reads as "`slash` is broken for every caller, so repairing
/// `PoSTest.rho:818` is not enough". It was not that. The probe's own SETUP sent
/// `makeDeployerId!(pk, *ret)` — two arguments — at a system process registered with `arity: 3`
/// (`rho_spec.rs:262-264`); `PoSTest.rho:79` calls it correctly as
/// `makeDeployerId!("deployerId", pk.hexToBytes(), *deployerIdCh)`. **A two-argument send at a
/// three-argument receive rests as data, silently — the same failure mode as `PoSTest.rho:818` and
/// `PoSTest.rho:704`,** reproduced inside the instrument built to study it.
///
/// That earns a permanent cell for two reasons:
///
/// 1. **It is why a blocked control must be DIAGNOSED, never relaxed.** The convenient move was to
///    weaken `assert!(current.is_completed())` to something the probe already satisfied. That would
///    have buried a bug in the instrument and published a false conclusion about `slash`.
/// 2. **`Blocked` does not localise the block.** [`SuiteOutcome::Blocked`] says the suite went quiet,
///    not where. This ladder is how the location was recovered: run each setup step *as* the whole
///    setup, with a body that needs nothing but the setup to answer.
///
/// Every rung is asserted rather than printed — a cell whose body is only `println!` passes
/// unconditionally, which is the vacuity this file exists to refuse.
#[tokio::test]
async fn the_rho_spec_runtime_answers_each_slash_setup_step_and_deployer_id_needs_three_arguments()
{
    /// One setup step, and whether it is expected to answer.
    struct Rung {
        name: &'static str,
        setup: &'static str,
        must_answer: bool,
    }

    // Needs nothing but the setup result, so "did it report" == "did the setup answer".
    const BODY: &str = r#"    rhoSpec!("assert", true, "setup answered", *ackCh)"#;

    let rungs = [
        Rung {
            name: "baseline: no setup at all",
            setup: r#"    retCh!(Nil)"#,
            must_answer: true,
        },
        Rung {
            name: "registry lookup rho:system:pos",
            setup: r#"    new PoSCh in { rl!(`rho:system:pos`, *PoSCh) |
      for (@(_, PoS) <- PoSCh) { retCh!(PoS) } }"#,
            must_answer: true,
        },
        Rung {
            name: "sys:test:authToken:make",
            setup: r#"    new makeSysAuthToken(`sys:test:authToken:make`), tokenCh in {
      makeSysAuthToken!(*tokenCh) | for (@token <- tokenCh) { retCh!(token) } }"#,
            must_answer: true,
        },
        Rung {
            name: "PoS!getActiveValidators",
            setup: r#"    new PoSCh, activeValidatorsCh in {
      rl!(`rho:system:pos`, *PoSCh) |
      for (@(_, PoS) <- PoSCh) {
        @PoS!("getActiveValidators", *activeValidatorsCh) |
        for (@av <- activeValidatorsCh) { retCh!(av.toList().nth(3)) } } }"#,
            must_answer: true,
        },
        Rung {
            name: "rho:crypto:keccak256Hash",
            setup: r#"    new computeHash(`rho:crypto:keccak256Hash`), hashCh in {
      computeHash!("bb".hexToBytes(), *hashCh) | for (@h <- hashCh) { retCh!(h) } }"#,
            must_answer: true,
        },
        Rung {
            name: "rho:test:casper:invalidBlocks:set",
            setup: r#"    new setInvalidBlocks(`rho:test:casper:invalidBlocks:set`), ibCh in {
      setInvalidBlocks!({"bb".hexToBytes() : "cc".hexToBytes()}, *ibCh) |
      for (_ <- ibCh) { retCh!(Nil) } }"#,
            must_answer: true,
        },
        Rung {
            name: "deployerId:make, THREE args (correct)",
            setup: r#"    new makeDeployerId(`rho:test:deployerId:make`), deployerIdCh in {
      makeDeployerId!("deployerId", "bb".hexToBytes(), *deployerIdCh) |
      for (@deployerId <- deployerIdCh) { retCh!(deployerId) } }"#,
            must_answer: true,
        },
        Rung {
            // ★★ THE RED RUNG, at the arity it refuses — and it was the probe's own bug.
            name: "deployerId:make, TWO args (the probe's own bug)",
            setup: r#"    new makeDeployerId(`rho:test:deployerId:make`), deployerIdCh in {
      makeDeployerId!("bb".hexToBytes(), *deployerIdCh) |
      for (@deployerId <- deployerIdCh) { retCh!(deployerId) } }"#,
            must_answer: false,
        },
    ];

    let mut disagreements: Vec<String> = Vec::new();
    for rung in &rungs {
        let out = run_suite(&one_test_suite(PROBE_TEST, rung.setup, BODY)).await;
        println!(
            "{:<48} must_answer={:<5} → {out:?}",
            rung.name, rung.must_answer
        );

        assert!(
            out.raise_text().is_none(),
            "★ every rung must either answer or REST — an arity mismatch does not raise. `{}` \
             raised: {out:?}",
            rung.name,
        );
        if out.is_completed() != rung.must_answer {
            disagreements.push(format!("{} → {out:?}", rung.name));
        }
    }

    assert!(
        disagreements.is_empty(),
        "★★ the RhoSpec runtime's answers for the slash setup no longer match what is recorded here. \
         Disagreements: {disagreements:#?}\n\
         ★ If the TWO-argument `deployerId:make` rung started ANSWERING, the system process gained a \
         two-argument form and this cell's story — that the probe's block was an arity mismatch in \
         the probe and not a defect in `slash` — needs re-deriving. If one of the `must_answer=true` \
         rungs STOPPED answering, then \
         `pos_slash_arity_mismatch_is_why_the_slashing_test_can_never_report`'s control is failing \
         for the same reason, and THAT is the block to diagnose first.",
    );
}
