//! # A guarded receive resting in the tuplespace must print with its guard
//!
//! The storage printer's whole design is a translation back into the language:
//! `Datum -> Send`, `WaitingContinuation -> Receive`, `concatenate_pars`, then
//! `PrettyPrinter`. That design is only sound if the printer can render what
//! the translation produces. For a `where` guard it could not, and the guard
//! was dropped **twice** on the way to the page:
//!
//! 1. `storage_printer::to_receives` read `wk.continuation.tagged_cont` and
//!    never `wk.continuation.guard`, which is on the same struct, so it built
//!    `Receive { …, condition: None }`.
//! 2. `PrettyPrinter` emitted no `where` token anywhere, so even a `Receive`
//!    that *did* carry a `condition` printed without it.
//!
//! The result was not an obviously-internal artefact. It was a **plausible and
//! wrong** term: `for (@x <- c where x > 5) { … }` rendered as
//! `for (@x <- c) { … }` — a receive the user did not write, with a strictly
//! weaker guard than the one they did, and nothing in the output signalling the
//! loss. Measured, before the repair:
//!
//! ```text
//!   "guarded"!(1) |
//!   …
//!   for( @{c2} <- @{"guarded"} ) {
//!     "out"!(d0)
//!   }
//! ```
//!
//! — a report that says the resting message *should* have been consumed.
//!
//! ## ★ Why these tests go through RSpace and not through the term
//!
//! The guard *does* reach RSpace: `Reduce::consume_inner` registers
//! `TaggedContinuation { tagged_cont: …, guard }`, and `RhoTypes.proto`
//! documents the field as *"lifted from `Receive.condition` when the
//! continuation is registered with rspace"*. So a test that builds a `Receive`
//! and prints it can be made to pass while the **resting** form still renders
//! wrong — which is exactly how this survived. Every test here therefore
//! evaluates a term, lets it **rest**, and reads the report.
//!
//! ## Anti-vacuity
//!
//! Before any assertion about rendered bytes, [`resting_guards`] asserts that
//! the fixture actually registered a live guard on a resting continuation. A
//! fixture that silently failed to install one would otherwise satisfy "the
//! report does not lose a guard" by having no guard to lose.

use models::rhoapi::Par;
use rholang::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};
use rholang::rust::interpreter::storage::storage_printer;
use rholang::rust::interpreter::test_utils::resources::with_runtime;

/// Every **live** guard resting on a continuation in the space right now.
///
/// "Live" is the predicate the decision sites use, not a weaker one:
/// `matcher::r#match::Matcher::check_commit` commits unconditionally when
/// `guard` is `None` *or* when it equals `Par::default()`, and
/// `Reduce::eval_receive` already collapses `Some(Par::default())` to `None`
/// before the continuation is registered. So a guard is live exactly when it is
/// `Some(g)` with `g != Par::default()`.
async fn resting_guards(runtime: &RhoRuntimeImpl) -> Vec<Par> {
    runtime
        .get_hot_changes()
        .await
        .values()
        .flat_map(|row| row.wks.iter())
        .filter_map(|wk| wk.continuation.guard.clone())
        .filter(|g| g != &Par::default())
        .collect()
}

/// Evaluate `term`, assert it produced no interpreter errors, and return the
/// storage report.
async fn rest_and_report(runtime: &mut RhoRuntimeImpl, term: &str) -> String {
    let result = runtime
        .evaluate_with_term(term)
        .await
        .expect("evaluation must not fail outright");
    assert!(
        result.errors.is_empty(),
        "term produced interpreter errors, so nothing rested for the report to show: {:?}",
        result.errors
    );
    storage_printer::pretty_print(runtime).await
}

/// The `for( … )` **header** line the fixture's receive occupies, and the
/// **body** line that follows it.
///
/// The report also contains every continuation the runtime installs at boot —
/// the registry bootstrap and one per system process — so an assertion has to
/// be pinned to this fixture's own receive. `channel` is the rendered channel
/// literal, which is unique to the fixture.
///
/// ⚠ Returning the pair rather than the header alone is deliberate: the
/// strongest available assertion is that the guard names its variable *exactly
/// as the body does*, and that comparison needs both lines.
fn receive_lines(report: &str, channel: &str) -> (String, String) {
    let lines: Vec<&str> = report.lines().collect();
    let header = lines
        .iter()
        .position(|line| line.contains("for(") && line.contains(channel))
        .unwrap_or_else(|| {
            panic!(
                "no `for( … )` line of the storage report mentions {channel}.\nREPORT:\n{report}"
            )
        });
    assert!(
        header + 1 < lines.len(),
        "the receive's header is the last line of the report, so it has no body to \
         compare against.\nREPORT:\n{report}"
    );
    (
        lines[header].trim().to_string(),
        lines[header + 1].trim().to_string(),
    )
}

/// The single variable name a body of the shape `"out"!(NAME)` sends.
///
/// This is what makes the guard assertion self-calibrating. The printer's
/// variable names are a function of position in the *whole* report — `base_id`
/// rotates once per receive bind, and the report carries every system process —
/// so a hard-coded `d0` would pin the fixture to how many system processes the
/// runtime happens to install. Reading the name out of the body instead pins
/// the assertion to the property that actually matters: **the guard renders in
/// the same de Bruijn environment as the body**.
fn variable_the_body_sends(body: &str) -> String {
    let open = body
        .find("!(")
        .unwrap_or_else(|| panic!("the body line is not a send: {body:?}"));
    let close = body
        .rfind(')')
        .unwrap_or_else(|| panic!("the body line is not a send: {body:?}"));
    let name = body[open + 2..close].trim().to_string();
    assert!(
        !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric()),
        "expected a single variable name in the body, got {name:?} from {body:?}"
    );
    name
}

// ===========================================================================
// THE RED — a resting guarded receive renders its guard
// ===========================================================================

/// `@"guarded"!(1)` cannot satisfy `x > 5`, so the guard refuses the COMM and
/// **both** the datum and the continuation come to rest. That is precisely the
/// state an operator reads the storage report to understand ("why is my message
/// still there?"), and it is the state in which the report used to answer with
/// a receive that would have consumed the message.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resting_guarded_receive_renders_its_where_clause() {
    with_runtime("storage-printer-guard-", |mut runtime| async move {
        let term = r#"@"guarded"!(1) | for (@x <- @"guarded" where x > 5) { @"out"!(x) }"#;
        let report = rest_and_report(&mut runtime, term).await;

        // ── ANTI-VACUITY: the fixture really did register a live guard ──────
        let guards = resting_guards(&runtime).await;
        assert_eq!(
            guards.len(),
            1,
            "the fixture must leave exactly one live guard resting in the space, or every \
             assertion below holds vacuously. Guards found: {guards:?}"
        );

        // ── the guard's own tokens, in the body's environment ───────────────
        let (header, body) = receive_lines(&report, r#"@{"guarded"}"#);
        let variable = variable_the_body_sends(&body);
        let expected = format!(" where {variable} > 5 ");

        assert!(
            header.contains(&expected),
            "the storage report of a GUARDED resting receive does not carry its guard.\n\
             expected the header to contain {expected:?}\n\
             header:                        {header:?}\n\
             body:                          {body:?}\n\
             The guard reached rspace (asserted above) and was dropped on the way to the \
             page.\nREPORT:\n{report}"
        );

        // The datum the guard refused is still resting, which is what makes the
        // rendered guard the answer to the operator's question.
        assert!(
            report.contains(r#""guarded"!(1)"#),
            "the guard-refused datum must still be resting.\nREPORT:\n{report}"
        );
    })
    .await;
}

/// ★ **A SEPARATE, PRE-EXISTING DEFECT, PINNED — not repaired here.**
///
/// A **joined** consume — `for (@x <- a & @y <- b) { … }`, one receipt over two
/// channels — never reaches the storage report at all, guard or no guard.
///
/// # The mechanism, and why the guard repair cannot touch it
///
/// `RhoRuntimeImpl::get_hot_changes` is `space.to_map()`, and
/// `InMemHotStore::to_map` (`rspace++/src/rspace/hot_store.rs`) builds its
/// result by iterating the **data** map, whose keys are single channels, and
/// looking each one up in the continuation map:
///
/// ```text
///   for (k, v) in data { row = Row { data: v, wks: all_continuations[k] } }
/// ```
///
/// A single-channel consume survives that because the consume attempt itself
/// calls `get_data(channel)`, which inserts an entry — possibly an *empty* one —
/// into the data map, so the channel is a key and its continuation is found.
/// A joined consume is keyed by the **two-element vector** `[a, b]`; no
/// single-channel data key can ever match it, so it is dropped before any
/// printing happens. The loss is upstream of `to_receives` and upstream of
/// `PrettyPrinter`, in a crate this change does not touch.
///
/// # Why pin it rather than fix it or stay silent
///
/// Fixing it changes what `get_hot_changes` returns for **every** caller — the
/// REPL, the CLI, and the FFI at `rholang/src/lib.rs` — which is a different
/// change with a different blast radius. Staying silent is worse: the guard
/// repair is easily mistaken for "guards now show up in the report", and for a
/// joined receive that is false for a reason that has nothing to do with
/// guards. This is the discipline `b98fa20a` used on the `match` target: make
/// the wrong behaviour a **pinned fact** so it is a decision rather than an
/// accident, and so the eventual repair has something to go red against.
///
/// The comparison against the **unguarded** twin is what makes the attribution
/// exact: both are equally invisible, so the guard is not what is being lost.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_joined_consume_never_reaches_the_storage_report_and_that_is_pinned() {
    with_runtime("storage-printer-joined-", |mut runtime| async move {
        let guarded = r#"for (@x <- @"gleft" & @y <- @"gright" where x + y > 10) { @"out"!(x) }"#;
        let report = rest_and_report(&mut runtime, guarded).await;
        for channel in [r#""gleft""#, r#""gright""#] {
            assert!(
                !report.contains(channel),
                "a joined consume reached the storage report on {channel}. That would be an \
                 IMPROVEMENT over the pinned behaviour — `to_map` keys continuations by the \
                 single channels of the data map — but it is not what this test records, so \
                 the pin and its explanation are now stale.\nREPORT:\n{report}"
            );
        }
    })
    .await;

    with_runtime("storage-printer-joined-plain-", |mut runtime| async move {
        // The attribution: identical term, no guard, equally invisible. The
        // loss is the JOIN, not the `where`.
        let unguarded = r#"for (@x <- @"pleft" & @y <- @"pright") { @"out"!(x) }"#;
        let report = rest_and_report(&mut runtime, unguarded).await;
        for channel in [r#""pleft""#, r#""pright""#] {
            assert!(
                !report.contains(channel),
                "an UNGUARDED joined consume reached the report while the guarded one did \
                 not, so the two are not lost for the same reason and the attribution above \
                 is wrong.\nREPORT:\n{report}"
            );
        }
    })
    .await;
}

// ===========================================================================
// THE CONTROLS — these must NOT discriminate
// ===========================================================================

/// ★ Control 1 — an **unguarded** resting receive must render byte-identically
/// before and after. The whole line is pinned, so a single moved byte fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unguarded_resting_receive_renders_byte_identically() {
    with_runtime("storage-printer-plain-", |mut runtime| async move {
        let term = r#"for (@x <- @"plain") { @"out"!(x) }"#;
        let report = rest_and_report(&mut runtime, term).await;

        let guards = resting_guards(&runtime).await;
        assert!(
            guards.is_empty(),
            "an unguarded receive must rest with no guard; found {guards:?}"
        );

        let (header, body) = receive_lines(&report, r#"@{"plain"}"#);
        let variable = variable_the_body_sends(&body);
        // Captured from the pre-repair run: `for( @{k2} <- @{"plain"} ) {`.
        // Only the rotating identifiers are computed; every other byte is
        // literal, including both single spaces inside the parentheses.
        let pattern = header
            .split_once(" <- ")
            .map(|(lhs, _)| lhs.to_string())
            .unwrap_or_else(|| panic!("unexpected header shape: {header:?}"));
        assert_eq!(
            header,
            format!(r#"{pattern} <- @{{"plain"}} ) {{"#),
            "the rendering of an UNGUARDED resting receive moved. It must be byte-identical \
             before and after the guard repair.\nREPORT:\n{report}"
        );
        assert_eq!(body, format!(r#""out"!({variable})"#));

        assert!(
            !report.contains("where"),
            "an unguarded resting receive grew a `where` clause.\nREPORT:\n{report}"
        );
    })
    .await;
}

/// ★ Control 2 — a **semantically empty** guard is not a guard. `where Nil`
/// commits unconditionally at `check_commit`, and `Reduce::eval_receive`
/// collapses it to `None` before registration; the report must agree with both
/// and print nothing. This is what keeps the byte movement confined to receives
/// whose behaviour is actually guarded.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_semantically_empty_guard_renders_as_no_guard() {
    with_runtime("storage-printer-nilguard-", |mut runtime| async move {
        let term = r#"for (@x <- @"nilguard" where Nil) { @"out"!(x) }"#;
        let report = rest_and_report(&mut runtime, term).await;

        let guards = resting_guards(&runtime).await;
        assert!(
            guards.is_empty(),
            "`where Nil` must not rest as a live guard; found {guards:?}"
        );

        let (header, body) = receive_lines(&report, r#"@{"nilguard"}"#);
        let variable = variable_the_body_sends(&body);
        let pattern = header
            .split_once(" <- ")
            .map(|(lhs, _)| lhs.to_string())
            .unwrap_or_else(|| panic!("unexpected header shape: {header:?}"));
        assert_eq!(
            header,
            format!(r#"{pattern} <- @{{"nilguard"}} ) {{"#),
            "a `where Nil` receive must render exactly as an unguarded one.\nREPORT:\n{report}"
        );
        assert_eq!(body, format!(r#""out"!({variable})"#));

        assert!(
            !report.contains("where"),
            "`where Nil` grew a rendered `where` clause.\nREPORT:\n{report}"
        );
    })
    .await;
}

/// ★ Control 3 — a resting **send** must be unaffected. `to_sends` has no
/// condition to read; if its rendering moves, the change reached too far.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_resting_send_is_unaffected() {
    with_runtime("storage-printer-send-", |mut runtime| async move {
        let term = r#"@"payload"!(1, "two", true)"#;
        let report = rest_and_report(&mut runtime, term).await;

        // Captured from the pre-repair run, byte for byte. A send carries no
        // rotating identifier, so the whole line is a literal.
        assert!(
            report.contains(r#""payload"!(1, "two", true)"#),
            "the rendering of a resting SEND moved. It has no condition to read, so a \
             change here means the repair reached further than the receive.\nREPORT:\n{report}"
        );
        assert!(
            !report.contains("where"),
            "a resting send grew a `where` clause.\nREPORT:\n{report}"
        );
    })
    .await;
}
