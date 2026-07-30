// See rholang/src/main/scala/coop/rchain/rholang/interpreter/storage/StoragePrinter.scala

use std::collections::BTreeSet;

use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, ListParWithRandom, Par, Receive, ReceiveBind, Send, TaggedContinuation,
};
use models::rust::rholang::implicits::concatenate_pars;
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};

use crate::rust::interpreter::pretty_printer::PrettyPrinter;
use crate::rust::interpreter::rest_diagnosis;
use crate::rust::interpreter::rho_runtime::{RhoRuntime, RhoRuntimeImpl};

pub async fn pretty_print(runtime: &RhoRuntimeImpl) -> String {
    let mapped = runtime.get_hot_changes().await;

    let pars: Vec<Par> = mapped
        .iter()
        .map(|(channels, row)| match row {
            Row { data, wks } if data.is_empty() && wks.is_empty() => Par::default(),
            Row { data, wks } if !data.is_empty() && wks.is_empty() => to_sends(data, channels),
            Row { data, wks } if data.is_empty() && !wks.is_empty() => to_receives(wks, channels),
            Row { data, wks } => {
                let sends = to_sends(data, channels);
                let receives = to_receives(wks, channels);
                concatenate_pars(sends, receives)
            }
        })
        .collect();

    if pars.is_empty() {
        "The space is empty. Note that top level terms that are not sends or receives are discarded.".to_string()
    } else {
        let combined_par = pars
            .into_iter()
            .fold(Par::default(), |acc, par| concatenate_pars(acc, par));

        let mut pretty_printer = PrettyPrinter::new();
        pretty_printer.build_string_from_message(&combined_par)
    }
}

/// ★ The resting terms, **with their reasons** — the companion to
/// [`pretty_print_unmatched_sends`], which renders the same terms and cannot say
/// why any of them is stuck.
///
/// # Why this is a NEW function and not a change to the two above
///
/// `pretty_print_unmatched_sends` renders a resting send as Rholang source, and
/// that string is a gRPC response body (`node/src/rust/api/repl_grpc_service.rs`).
/// Appending a diagnosis to it would change an API's payload for every existing
/// caller. Silence is a defect in the *surface*, not in that function's contract,
/// so the reason gets its own surface.
///
/// # ⚠ What it is safe to call this from
///
/// It reads `get_hot_changes()`, whose implementation is `HotStore::to_map` — a
/// read-lock clone with no history fill and no write. It returns a `String` and
/// not a `Result`, so no caller can `?` on it and none can make a deploy fail
/// with it. It writes nothing to the store, nothing to `EvaluateResult`, and
/// nothing to the event log. See
/// [`crate::rust::interpreter::rest_diagnosis`] §4 for the full argument and for
/// the reason it is deliberately **not** called from the deploy path.
pub async fn pretty_print_rest_diagnosis(runtime: &RhoRuntimeImpl) -> String {
    let snapshot = runtime.get_hot_changes().await;
    rest_diagnosis::report(&rest_diagnosis::diagnose(&snapshot))
}

pub async fn pretty_print_unmatched_sends(runtime: &RhoRuntimeImpl) -> String {
    let mapped = runtime.get_hot_changes().await;

    let pars: Vec<Par> = mapped
        .iter()
        .filter_map(|(channels, row)| {
            if !row.data.is_empty() {
                Some(to_sends(&row.data, channels))
            } else {
                None
            }
        })
        .collect();

    if pars.is_empty() {
        "The space is empty. Note that top level terms that are not sends or receives are discarded.".to_string()
    } else {
        let combined_par = pars
            .into_iter()
            .fold(Par::default(), |acc, par| concatenate_pars(acc, par));

        let mut pretty_printer = PrettyPrinter::new();
        pretty_printer.build_string_from_message(&combined_par)
    }
}

fn to_sends(data: &Vec<Datum<ListParWithRandom>>, channels: &Vec<Par>) -> Par {
    let mut sends: Vec<Send> = Vec::new();

    for datum in data {
        for channel in channels {
            sends.push(Send {
                chan: Some(channel.clone()),
                data: datum.a.pars.clone(),
                persistent: datum.persist,
                locally_free: Vec::new(),
                connective_used: false,
            });
        }
    }

    sends
        .into_iter()
        .fold(Par::default(), |mut acc, send| acc.prepend_send(send))
}

/// A resting continuation, back in the language it was written in.
///
/// ⚠ **Every field of the resting record that the language can express has to
/// be read here.** This function used to read `wk.continuation.tagged_cont` and
/// nothing else, while `wk.continuation.guard` — the `where` clause, on the
/// *same struct* — was left behind. `RhoTypes.proto` says what that field is:
/// *"Optional `where`-clause guard, lifted from `Receive.condition` when the
/// continuation is registered with rspace."* Dropping it did not produce an
/// obviously-internal artefact; it produced a **plausible and wrong** term.
/// `for (@x <- c where x > 5) { … }` came back as `for (@x <- c) { … }`: a
/// receive the author never wrote, carrying a strictly weaker guard, with
/// nothing in the output to signal the loss. An operator reading the report to
/// find out why a message was still resting was shown a receive that would have
/// consumed it.
///
/// The guard travels back the way it came: `Receive.condition` ->
/// (`Reduce::consume_inner`) -> `TaggedContinuation.guard` -> here ->
/// `Receive.condition`. See `pretty_printer::receive_guard` for the other half
/// — the printer had no `where` token at all until this was repaired.
fn to_receives(
    wks: &Vec<WaitingContinuation<BindPattern, TaggedContinuation>>,
    channels: &Vec<Par>,
) -> Par {
    let mut receives: Vec<Receive> = Vec::new();

    for wk in wks {
        let patterns = &wk.patterns;
        let continuation = &wk.continuation;
        let persist = wk.persist;
        let peeks: &BTreeSet<i32> = &wk.peeks;
        // The `where` clause, read back off the record it was lifted onto. Both
        // arms below take it: a guard is a property of the *consume*, not of
        // which flavour of continuation the consume installed, so a
        // `ScalaBodyRef` continuation carrying one would be losing it here for
        // exactly the same reason a `ParBody` one was.
        let guard = continuation.guard.clone();

        let mut receive_binds: Vec<ReceiveBind> = Vec::new();
        for (i, pattern) in patterns.iter().enumerate() {
            if i < channels.len() {
                let channel = &channels[i];
                receive_binds.push(ReceiveBind {
                    patterns: pattern.patterns.clone(),
                    source: Some(channel.clone()),
                    remainder: pattern.remainder.clone(),
                    free_count: pattern.free_count,
                });
            }
        }

        match &continuation.tagged_cont {
            Some(TaggedCont::ParBody(p)) => {
                let free_count_sum = patterns.iter().map(|p| p.free_count).sum();

                receives.push(Receive {
                    binds: receive_binds,
                    body: p.body.clone(),
                    persistent: persist,
                    peek: !peeks.is_empty(),
                    bind_count: free_count_sum,
                    locally_free: Vec::new(),
                    connective_used: false,
                    condition: guard,
                });
            }
            _ => {
                receives.push(Receive {
                    binds: receive_binds,
                    body: Some(Par::default()),
                    persistent: persist,
                    peek: false,
                    bind_count: 0,
                    locally_free: Vec::new(),
                    connective_used: false,
                    condition: guard,
                });
            }
        }
    }

    receives
        .into_iter()
        .fold(Par::default(), |mut acc, receive| {
            acc.prepend_receive(receive)
        })
}
