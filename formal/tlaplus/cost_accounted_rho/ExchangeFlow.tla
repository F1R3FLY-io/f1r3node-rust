-------------------------------- MODULE ExchangeFlow --------------------------------
(****************************************************************************)
(* Cost-Accounted Rho Stage D: the blessed conserving 1:1 token Exchange     *)
(* (spec "Fee conversion" cost-accounted-rho.tex:3061-3084 / DR-4).          *)
(*                                                                          *)
(*     Exchange(c, v) = for (t_c <- n_c & t_v <- n_v) {                       *)
(*                        n_c ! drop t_v | n_v ! drop t_c }                   *)
(*                                                                          *)
(* A persistent JOIN over two ordinary carrier channels n_c, n_v: it          *)
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
(*   - Inv_StackIdentityAndOrder / Inv_StackCellConservation: first-class      *)
(*       resource-stack payloads are exchanged exactly, with no retagging,     *)
(*       reordering, minting, or loss.                                          *)
(*                                                                          *)
(* This is the TLA+ companion of Rocq Exchange.v                              *)
(* (exchange_conserves_per_channel / exchange_total_conserved /               *)
(*  exchange_requires_both_inputs / exchange_preserves_stack_identity_and_order)*)
(* and the corresponding Rust exact-stack transport regressions.               *)
(****************************************************************************)

EXTENDS Integers, FiniteSets, Sequences

CONSTANTS
    InitC,    \* Nat: the initial count datum on the c-carrier
    InitV,    \* Nat: the initial count datum on the v-carrier
    RequireBoth

ASSUME InitC \in Nat /\ InitV \in Nat /\ RequireBoth \in BOOLEAN

VARIABLES
    cDatum,   \* the count datum currently on the c-carrier (the Int it holds)
    vDatum,   \* the count datum currently on the v-carrier
    cPresent, \* BOOLEAN: a datum is present on the c-carrier
    vPresent, \* BOOLEAN: a datum is present on the v-carrier
    cStack,
    vStack,
    swapped,  \* BOOLEAN: the join has fired
    firedWithBoth

vars == <<cDatum, vDatum, cPresent, vPresent, cStack, vStack, swapped, firedWithBoth>>

InitCStack == <<InitC, InitC + 1>>
InitVStack == <<InitV, InitV + 1>>

TypeOK ==
    /\ cDatum \in Nat
    /\ vDatum \in Nat
    /\ cPresent \in BOOLEAN
    /\ vPresent \in BOOLEAN
    /\ cStack \in Seq(Nat)
    /\ vStack \in Seq(Nat)
    /\ swapped \in BOOLEAN
    /\ firedWithBoth \in BOOLEAN

Init ==
    /\ cDatum   = InitC
    /\ vDatum   = InitV
    /\ cPresent = TRUE      \* both carriers seeded with one datum each
    /\ vPresent = TRUE
    /\ cStack   = InitCStack
    /\ vStack   = InitVStack
    /\ swapped  = FALSE
    /\ firedWithBoth = TRUE

(*--------------------------------------------------------------------------*)
(* The join FIRES (the only action) iff BOTH carriers carry a datum (DR-4).   *)
(* On firing, it consumes one datum from each carrier and re-emits each on     *)
(* the OTHER carrier — the c-carrier now holds the former v-datum and vice      *)
(* versa (1:1 swap). Each carrier still holds exactly one datum afterwards      *)
(* (one consumed, one produced), and the total cDatum + vDatum is unchanged.    *)
(*--------------------------------------------------------------------------*)
Swap ==
    /\ IF RequireBoth
          THEN cPresent = TRUE /\ vPresent = TRUE
          ELSE cPresent = TRUE \/ vPresent = TRUE
    /\ swapped  = FALSE
    /\ cDatum'   = vDatum       \* n_c ! drop t_v
    /\ vDatum'   = cDatum       \* n_v ! drop t_c
    /\ cStack'   = vStack
    /\ vStack'   = cStack
    /\ cPresent' = TRUE         \* one consumed + one produced ⇒ still present
    /\ vPresent' = TRUE
    /\ swapped'  = TRUE
    /\ firedWithBoth' = (cPresent /\ vPresent)

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
    /\ UNCHANGED <<cDatum, vDatum, vPresent, cStack, vStack, swapped, firedWithBoth>>

DrainV ==
    /\ swapped  = FALSE
    /\ vPresent = TRUE
    /\ vPresent' = FALSE
    /\ UNCHANGED <<cDatum, vDatum, cPresent, cStack, vStack, swapped, firedWithBoth>>

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
(* carried a datum at fire time. [firedWithBoth] records that pre-state fact;    *)
(* In particular a one-sided drained state (¬cPresent ∨ ¬vPresent before the     *)
(* swap) can NEVER reach [swapped = TRUE] — Swap is disabled there. So no        *)
(* one-sided carrier triggers a swap/credit (no one-sided mint).                 *)
(*--------------------------------------------------------------------------*)
Inv_RequiresBothInputs ==
    swapped => firedWithBoth

(*--------------------------------------------------------------------------*)
(* The swap is value-exact: once fired, the c-carrier holds the original         *)
(* v-value and vice versa (the spec's "swaps one c-token for one v-token").      *)
(*--------------------------------------------------------------------------*)
Inv_SwapExchangesValues ==
    swapped => (cDatum = InitV /\ vDatum = InitC)

Inv_StackIdentityAndOrder ==
    IF swapped
      THEN cStack = InitVStack /\ vStack = InitCStack
      ELSE cStack = InitCStack /\ vStack = InitVStack

Inv_StackCellConservation ==
    Len(cStack) + Len(vStack) = Len(InitCStack) + Len(InitVStack)

=============================================================================
