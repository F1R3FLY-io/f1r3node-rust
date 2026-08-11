// Cost-Accounted Rho — Rust runtime budget conservation, Verus-verified.
//
// A correspondence→PROOF cross-witness for the accounting runtime's escrow
// reconciliation: splitting an escrow into a charged amount and a refund conserves
// the total, and the refund is bounded by the escrow. This is the Verus image of
// the Rocq CASettlement.charged_plus_refund_eq_escrow / post_evaluation_settlement
// guarantees on the functional reconciliation core (reconcile / reserve_canonical_
// with_cost). Pure (overflow-free nat) functional core; the lock-free AtomicU64/CAS
// linearizability is the Iris leg.
use vstd::prelude::*;

verus! {

// The refund left when `charged` is debited from `escrow`.
spec fn refund(escrow: nat, charged: nat) -> nat
    recommends charged <= escrow,
{
    (escrow - charged) as nat
}

// Conservation: charged + refund == escrow (no funds created or destroyed).
proof fn budget_split_conserves(escrow: nat, charged: nat)
    requires charged <= escrow,
    ensures charged + refund(escrow, charged) == escrow,
{
}

// The refund never exceeds the escrow.
proof fn refund_bounded(escrow: nat, charged: nat)
    requires charged <= escrow,
    ensures refund(escrow, charged) <= escrow,
{
}

// Monotone debit: charging more never increases the refund.
proof fn debit_monotone(escrow: nat, c1: nat, c2: nat)
    requires c1 <= c2, c2 <= escrow,
    ensures refund(escrow, c2) <= refund(escrow, c1),
{
}

} // verus!

fn main() {}
