// Cost-Accounted Rho — Rust runtime budget conservation, Verus-verified.
//
// A correspondence→PROOF cross-witness for protocol-4 reservation settlement:
// splitting a fixed reservation into an exact debit and a refund conserves the
// total, and the refund is bounded by the reservation. This is the Verus image of
// the Rocq Settlement.debit_plus_refund_eq_reservation /
// post_evaluation_settlement_no_mint guarantees on the pure, overflow-free
// reconciliation core. The lock-free AtomicU64/CAS linearizability is the Iris leg.
use vstd::prelude::*;

verus! {

// The refund left when `debit` is removed from `reservation`.
spec fn refund(reservation: nat, debit: nat) -> nat
    recommends debit <= reservation,
{
    (reservation - debit) as nat
}

// Conservation: exact debit plus refund equals the fixed reservation.
proof fn budget_split_conserves(reservation: nat, debit: nat)
    requires debit <= reservation,
    ensures debit + refund(reservation, debit) == reservation,
{
}

// The refund never exceeds the reservation.
proof fn refund_bounded(reservation: nat, debit: nat)
    requires debit <= reservation,
    ensures refund(reservation, debit) <= reservation,
{
}

// Monotone debit: charging more never increases the refund.
proof fn debit_monotone(reservation: nat, c1: nat, c2: nat)
    requires c1 <= c2, c2 <= reservation,
    ensures refund(reservation, c2) <= refund(reservation, c1),
{
}

} // verus!

fn main() {}
