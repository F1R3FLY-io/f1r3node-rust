//! Phase-2 splitter infrastructure (combined-cell tokens, R3/R5).
//!
//! A combined-cell token `(s1*…*sn):S` is a send on the compound channel
//! `Σ⟦s1*…*sn⟧` (minted by a `purse` whose layer is a `Compound` signature).
//! The fuel gates listen on the **component** channels, so a combined token
//! would otherwise sit unconsumed. The **splitter** is a persistent contract
//! that consumes a combined token and re-emits the **chained split form** the
//! gates thread through (see `docs/cost-accounting/phase2-faithfulness.md`):
//!
//! ```text
//! for( c <= Σ⟦s1*…*sn⟧ ){ Σ⟦s1⟧!( Σ⟦s2⟧!( … Σ⟦sn⟧!( *c ) … ) ) }
//! ```
//!
//! The remaining stack `S` (the payload `*c`) rides the innermost send and is
//! released exactly once, matching `~> R, S`. The splitter is emitted inline
//! with each compound purse layer (it listens on the *global* compound
//! channel, so one suffices for all combined tokens of that signature;
//! duplicates are harmless — a persistent receive fires once per token).

use models::create_bit_vector;
use models::rhoapi::{Par, Receive, ReceiveBind};
use models::rust::utils::{new_boundvar_par, new_freevar_par, new_send_par};

use super::ir::Sig;
use super::sig::supply_channel;
use crate::rust::interpreter::util::filter_and_adjust_bitset;

/// Build the persistent splitter for a compound signature: a replicated
/// receive on the combined channel that re-emits the chained split form. The
/// result is a closed `Par` (the combined channel and all component channels
/// are closed; the only bound variable, `c`, is consumed by the receive).
pub fn build_splitter(compound: &Sig) -> Par {
    let combined_channel = supply_channel(compound);
    let components = compound.atoms();

    // body = Σ⟦s1⟧!( … Σ⟦sn⟧!( *c ) … ), with *c = BoundVar(0).
    let mut body = new_boundvar_par(0, create_bit_vector(&[0]), false);
    for component in components.iter().rev() {
        let channel = supply_channel(component);
        let lf = body.locally_free.clone();
        let cu = body.connective_used;
        body = new_send_par(channel, vec![body], false, lf.clone(), cu, lf, cu);
    }

    let receive = Receive {
        binds: vec![ReceiveBind {
            patterns: vec![new_freevar_par(0, Vec::new())],
            source: Some(combined_channel),
            remainder: None,
            free_count: 1,
        }],
        body: Some(body.clone()),
        persistent: true,
        peek: false,
        bind_count: 1,
        locally_free: filter_and_adjust_bitset(body.locally_free.clone(), 1),
        connective_used: body.connective_used,
        condition: None,
    };
    Par::default().prepend_receive(receive)
}
