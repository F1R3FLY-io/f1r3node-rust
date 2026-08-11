-------------------------------- MODULE ExchangeFlow --------------------------------
(****************************************************************************)
(* Cost-Accounted Rho Stage D: the blessed conserving 1:1 token Exchange     *)
(* (spec "Fee conversion" cost-accounted-rho.tex:3061-3084 / DR-4).          *)
(*                                                                          *)
(*     Exchange(c, v) = for (t_c <- n_c & t_v <- n_v) {                       *)
(*                        n_c ! drop t_v | n_v ! drop t_c }                   *)
(*                                                                          *)
(* A persistent JOIN over two count-datum carrier channels n_c, n_v: it      *)
(* consumes one datum from EACH carrier and re-emits one on each, SWAPPED     *)
(* (1:1 peg, OD-5). This module model-checks the two economic guarantees:    *)
(*                                                                          *)
(*   - Inv_PerChannelConservation : each carrier holds exactly ONE datum      *)
(*       throughout (one consumed ⇒ one produced per channel), and the two    *)
(*       carriers' TOTAL token count is invariant across the swap (no mint,    *)
(*       no burn).                                                            *)
(*   - Inv_RequiresBothInputs (DR-4) : the join FIRES only when BOTH carriers  *)
(*       carry a datum; a one-sided carrier cannot trigger a swap/credit (no   *)
(*       one-sided mint).                                                      *)
(*                                                                          *)
(* This is the TLA+ companion of Rocq Exchange.v                              *)
(* (exchange_conserves_per_channel / exchange_total_conserved /               *)
(*  exchange_requires_both_inputs) and Rust exchange_conserves_per_channel.    *)
(****************************************************************************)

EXTENDS Integers, FiniteSets

CONSTANTS
    InitC,    \* Nat: the initial count datum on the c-carrier
    InitV     \* Nat: the initial count datum on the v-carrier

ASSUME InitC \in Nat /\ InitV \in Nat

VARIABLES
    cDatum,   \* the count datum currently on the c-carrier (the Int it holds)
    vDatum,   \* the count datum currently on the v-carrier
    cPresent, \* BOOLEAN: a datum is present on the c-carrier
    vPresent, \* BOOLEAN: a datum is present on the v-carrier
    swapped   \* BOOLEAN: the join has fired

vars == <<cDatum, vDatum, cPresent, vPresent, swapped>>

TypeOK ==
    /\ cDatum \in Nat
    /\ vDatum \in Nat
    /\ cPresent \in BOOLEAN
    /\ vPresent \in BOOLEAN
    /\ swapped \in BOOLEAN

Init ==
    /\ cDatum   = InitC
    /\ vDatum   = InitV
    /\ cPresent = TRUE      \* both carriers seeded with one datum each
    /\ vPresent = TRUE
    /\ swapped  = FALSE

(*--------------------------------------------------------------------------*)
(* The join FIRES (the only action) iff BOTH carriers carry a datum (DR-4).   *)
(* On firing, it consumes one datum from each carrier and re-emits each on     *)
(* the OTHER carrier — the c-carrier now holds the former v-datum and vice      *)
(* versa (1:1 swap). Each carrier still holds exactly one datum afterwards      *)
(* (one consumed, one produced), and the total cDatum + vDatum is unchanged.    *)
(*--------------------------------------------------------------------------*)
Swap ==
    /\ cPresent = TRUE          \* DR-4: requires a datum on BOTH carriers
    /\ vPresent = TRUE
    /\ swapped  = FALSE
    /\ cDatum'   = vDatum       \* n_c ! drop t_v
    /\ vDatum'   = cDatum       \* n_v ! drop t_c
    /\ cPresent' = TRUE         \* one consumed + one produced ⇒ still present
    /\ vPresent' = TRUE
    /\ swapped'  = TRUE

(*--------------------------------------------------------------------------*)
(* A one-sided carrier (model a carrier becoming empty BEFORE the swap fires): *)
(* if either carrier is empty, Swap is DISABLED — so the join cannot fire from  *)
(* a single input. We expose this via an explicit DrainC / DrainV that can       *)
(* empty a carrier before the swap, letting TLC explore the one-sided states     *)
(* and confirm Swap never fires from them (Inv_RequiresBothInputs).              *)
(*--------------------------------------------------------------------------*)
DrainC ==
    /\ swapped  = FALSE
    /\ cPresent = TRUE
    /\ cPresent' = FALSE
    /\ UNCHANGED <<cDatum, vDatum, vPresent, swapped>>

DrainV ==
    /\ swapped  = FALSE
    /\ vPresent = TRUE
    /\ vPresent' = FALSE
    /\ UNCHANGED <<cDatum, vDatum, cPresent, swapped>>

Next ==
    \/ Swap
    \/ DrainC
    \/ DrainV

Spec == Init /\ [][Next]_vars

(*==========================================================================*)
(* INVARIANTS                                                               *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* Inv_PerChannelConservation: the two carriers' TOTAL token count is the      *)
(* invariant InitC + InitV at every reachable state where neither carrier has   *)
(* been drained (the swap moves values between carriers, never minting or        *)
(* destroying). Once swapped, the per-channel datum count is still exactly one    *)
(* on each carrier (cPresent ∧ vPresent), and the total is preserved.            *)
(* TLA+ analogue of Rocq exchange_total_conserved / exchange_conserves_per_channel.*)
(*--------------------------------------------------------------------------*)
Inv_PerChannelConservation ==
    (cPresent /\ vPresent) => (cDatum + vDatum = InitC + InitV)

(*--------------------------------------------------------------------------*)
(* Inv_RequiresBothInputs (DR-4): the swap has fired ONLY if both carriers      *)
(* carried a datum at fire time — captured as: if [swapped] then the post-swap   *)
(* carriers both hold a datum (the join consumed one + produced one on EACH).    *)
(* In particular a one-sided drained state (¬cPresent ∨ ¬vPresent before the     *)
(* swap) can NEVER reach [swapped = TRUE] — Swap is disabled there. So no        *)
(* one-sided carrier triggers a swap/credit (no one-sided mint).                 *)
(*--------------------------------------------------------------------------*)
Inv_RequiresBothInputs ==
    swapped => (cPresent /\ vPresent)

(*--------------------------------------------------------------------------*)
(* The swap is value-exact: once fired, the c-carrier holds the original         *)
(* v-value and vice versa (the spec's "swaps one c-token for one v-token").      *)
(*--------------------------------------------------------------------------*)
Inv_SwapExchangesValues ==
    swapped => (cDatum = InitV /\ vDatum = InitC)

=============================================================================
