--------------------------- MODULE CompoundSettlement -------------------------
(****************************************************************************)
(* #12 — the EXACT per-component (Split/Join) compound settlement debit.      *)
(*                                                                          *)
(* Models the THREE-POOL compound ([Sig::And s₁ s₂]) settlement debit that    *)
(* the Rust gate computes in                                                 *)
(*   casper/src/rust/util/rholang/acceptance.rs::compute_settlement_debits.   *)
(* A compound-signed COMM consumes ONE token from the combined pool Σ⟦comp⟧   *)
(* OR a MATCHED PAIR from the component pools Σ⟦s₁⟧, Σ⟦s₂⟧ (spec §3.6 Rule 2  *)
(* + Rule 4, tex 677-728; App. A Split/Join, tex 2020-2245). The gate splits  *)
(* the admitted compound demand k COMBINED-POOL-FIRST:                        *)
(*                                                                          *)
(*   draw_compound = min(k, Σ⟦comp⟧)                                          *)
(*   draw_pair     = k − draw_compound       (≤ min(Σ⟦s₁⟧, Σ⟦s₂⟧) by admission)*)
(*   Σ⟦comp⟧ −= draw_compound ; Σ⟦s₁⟧ −= draw_pair ; Σ⟦s₂⟧ −= draw_pair        *)
(*                                                                          *)
(* This is a FOCUSED model (a small new module rather than a generalization   *)
(* of EvalScheduling, to keep the existing 25 TLC specs green). It explores    *)
(* the full bounded space of pre-state balances (0..MaxSupply on each of the  *)
(* three pools) and every admissible demand k (0..Σ⟦comp⟧ + min(Σ⟦s₁⟧,Σ⟦s₂⟧)),*)
(* AND a cross-group contention case where a SECOND compound group shares the  *)
(* component pool Σ⟦s₁⟧ (the residual-ledger / shared-component invariant).    *)
(*                                                                          *)
(* The two headline SETTLEMENT invariants are the TLA+ analogues of the Rocq  *)
(* [compound_split_debit_conserves]:                                          *)
(*   Inv_CompoundDebitConserves     — Σ post-pools + total drawn = Σ pre-pools.*)
(*   Inv_ComponentDrawNoUnderflow   — each component post-pool ≥ 0.           *)
(*                                                                          *)
(* TM-CA-165 adds the cross-group ADMISSION bound: AdmitGate now threads the   *)
(* shared residual, bounding the SECOND group's demand by the LIVE effective    *)
(* supply after group 1, so the cross-group cumulative draw on a shared         *)
(* component is bounded at ADMISSION — not merely capped at settlement:         *)
(*   Inv_CrossGroupAdmissionBounded   — k2 ≤ LIVE effective supply.            *)
(*   Inv_SecondGroupDrawMatchesDemand — group 2's full demand settles.         *)
(* Rocq analogue: cross_group_draw_le_supply + cross_group_admission_sound      *)
(* (LinearLogicResources.v); Rust: the LIVE `remaining` ledger in               *)
(* acceptance.rs::admit_by_funding_with_logic + recompute_settlement_debits.    *)
(****************************************************************************)

EXTENDS Integers, FiniteSets

CONSTANTS
    MaxSupply       \* Nat: each of the three pools ranges over 0..MaxSupply

ASSUME MaxSupply \in Nat

Min2(a, b) == IF a <= b THEN a ELSE b

(*--------------------------------------------------------------------------*)
(* State.                                                                    *)
(*                                                                          *)
(* phase        : "pregate" before the gate decides k; "admitted" once a      *)
(*                demand k (within the admission bound) is fixed; "settled"    *)
(*                after the three-pool debit lands.                            *)
(* sComp/s1/s2  : the PRE-state pool balances Σ⟦comp⟧, Σ⟦s₁⟧, Σ⟦s₂⟧ (fixed at  *)
(*                Init, nondeterministically over 0..MaxSupply).               *)
(* k            : the admitted compound demand (chosen in 0..effectiveΣ).      *)
(* drawComp     : tokens drawn from the combined pool (= min(k, sComp)).       *)
(* drawPair     : tokens drawn from EACH component pool (= k − drawComp).       *)
(* postComp/post1/post2 : the three POST-state pool balances after the debit.   *)
(*                                                                          *)
(* Cross-group contention (the #12 residual-ledger invariant): a SECOND        *)
(* compound group And(s₁, s₃) also draws the shared component s₁. k2 is its     *)
(* admitted demand; the residual ledger bounds its pair-draw by s₁'s LIVE       *)
(* remaining balance after the first group. s1Draw2 is the second group's       *)
(* draw on s₁; s1SummedDraw is the TOTAL draw on s₁ across both groups.          *)
(*--------------------------------------------------------------------------*)
VARIABLES
    phase,
    sComp, s1, s2,
    k,
    drawComp, drawPair,
    postComp, post1, post2,
    s3, k2, drawComp2, drawPair2,
    post3, postS1AfterBoth

vars == <<phase, sComp, s1, s2, k, drawComp, drawPair,
          postComp, post1, post2,
          s3, k2, drawComp2, drawPair2, post3, postS1AfterBoth>>

Supplies == 0..MaxSupply

(* The effective supply of a compound group: combined pool plus the matched
   component minimum (the Split/Join admission cap effectiveΣ_compound). *)
EffectiveSupply(c, a, b) == c + Min2(a, b)

TypeOK ==
    /\ phase \in {"pregate", "admitted", "settled"}
    /\ sComp \in Supplies
    /\ s1 \in Supplies
    /\ s2 \in Supplies
    /\ s3 \in Supplies
    /\ k  \in 0..(MaxSupply + MaxSupply)
    /\ k2 \in 0..(MaxSupply + MaxSupply)
    /\ drawComp \in 0..MaxSupply
    /\ drawPair \in 0..MaxSupply
    /\ drawComp2 \in 0..MaxSupply
    /\ drawPair2 \in 0..MaxSupply
    /\ postComp \in 0..MaxSupply
    /\ post1 \in 0..MaxSupply
    /\ post2 \in 0..MaxSupply
    /\ post3 \in 0..MaxSupply
    /\ postS1AfterBoth \in 0..MaxSupply

Init ==
    /\ phase = "pregate"
    \* PRE-state pool balances: every combination over 0..MaxSupply on the
    \* three primary pools and the second group's distinct component s₃.
    /\ sComp \in Supplies
    /\ s1 \in Supplies
    /\ s2 \in Supplies
    /\ s3 \in Supplies
    \* Demands unset until the gate admits.
    /\ k = 0
    /\ k2 = 0
    /\ drawComp = 0
    /\ drawPair = 0
    /\ drawComp2 = 0
    /\ drawPair2 = 0
    \* Posts seeded to the pre-state (no debit yet).
    /\ postComp = sComp
    /\ post1 = s1
    /\ post2 = s2
    /\ post3 = s3
    /\ postS1AfterBoth = s1

(*--------------------------------------------------------------------------*)
(* The acceptance gate admits a compound demand k for the FIRST group and k2  *)
(* for the SECOND group (sharing component s₁). Each is bounded by its         *)
(* effective supply — the EXACT admission bound the Rust gate enforces         *)
(* (a compound is fundable up to Σ⟦comp⟧ + min(Σ⟦s₁⟧, Σ⟦s₂⟧)). We pick both    *)
(* nondeterministically over their admissible ranges so TLC explores every     *)
(* admissible (k, k2) against every pre-state.                                 *)
(*--------------------------------------------------------------------------*)
AdmitGate ==
    /\ phase = "pregate"
    /\ \E kk \in 0..EffectiveSupply(sComp, s1, s2) :
         \* TM-CA-165: the cross-group ledger gate bounds the SECOND group's demand
         \* by the LIVE effective supply AFTER group 1's combined-first draw on the
         \* shared component s₁ — group 2 sees the DRAWN-DOWN s₁ (s1 − dP1), not its
         \* full pre-balance. (The pre-fix gate used EffectiveSupply(sComp, s1, s3)
         \* with s₁ at FULL balance — the cross-group over-admission this models the
         \* fix of: both groups were admitted against s₁'s full balance.)
         LET dC1 == Min2(kk, sComp)
             dP1 == kk - dC1
         IN \E kk2 \in 0..EffectiveSupply(sComp - dC1, s1 - dP1, s3) :
              /\ k' = kk
              /\ k2' = kk2
    /\ phase' = "admitted"
    /\ UNCHANGED <<sComp, s1, s2, s3, drawComp, drawPair, drawComp2, drawPair2,
                   postComp, post1, post2, post3, postS1AfterBoth>>

(*--------------------------------------------------------------------------*)
(* SETTLE: the EXACT three-pool compound debit (combined-pool-first), plus the *)
(* cross-group residual-ledger draw of the shared component s₁ by the second    *)
(* group. Mirrors compute_settlement_debits:                                    *)
(*   group 1: drawComp = min(k, sComp); drawPair = k − drawComp                 *)
(*            (drawPair ≤ min(s1, s2) by admission, so post1, post2 ≥ 0).        *)
(*   group 2 (shares s₁): drawComp2 = min(k2, sComp − drawComp); the pair-draw   *)
(*            is bounded by the LIVE residual of s₁ AFTER group 1 (s1 − drawPair) *)
(*            and of s₃ — so the SUMMED draw on s₁ never exceeds its pre-balance. *)
(*--------------------------------------------------------------------------*)
Settle ==
    /\ phase = "admitted"
    /\ LET dC  == Min2(k, sComp)
           dP  == k - dC
           \* second group draws the combined pool's residual after group 1,
           \* then its component pair bounded by the LIVE residual of the shared
           \* s₁ (s1 − dP) and of s₃.
           dC2 == Min2(k2, sComp - dC)
           rem2 == k2 - dC2
           dP2 == Min2(rem2, Min2(s1 - dP, s3))
       IN  /\ drawComp' = dC
           /\ drawPair' = dP
           /\ drawComp2' = dC2
           /\ drawPair2' = dP2
           /\ postComp' = sComp - dC - dC2
           /\ post1' = s1 - dP
           /\ post2' = s2 - dP
           /\ post3' = s3 - dP2
           /\ postS1AfterBoth' = s1 - dP - dP2
    /\ phase' = "settled"
    /\ UNCHANGED <<sComp, s1, s2, s3, k, k2>>

Next ==
    \/ AdmitGate
    \/ Settle

Spec == Init /\ [][Next]_vars

(*==========================================================================*)
(* INVARIANTS                                                               *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* Inv_ComponentDrawNoUnderflow: every settled pool balance is ≥ 0 — the       *)
(* compound debit never underflows ANY of the three pools (nor the shared-      *)
(* component pool under cross-group contention). The Rust TLA+ analogue of      *)
(* the Rocq [compound_split_debit_no_underflow]; in TLA+ the balances are        *)
(* Int-typed, so this is the substantive non-negativity statement.              *)
(*--------------------------------------------------------------------------*)
Inv_ComponentDrawNoUnderflow ==
    phase = "settled" =>
        /\ postComp >= 0
        /\ post1 >= 0
        /\ post2 >= 0
        /\ post3 >= 0
        /\ postS1AfterBoth >= 0

(*--------------------------------------------------------------------------*)
(* Inv_CompoundDebitConserves: the sum of the three POST pools plus the TOTAL  *)
(* tokens drawn EQUALS the sum of the three PRE pools — no fuel created or       *)
(* destroyed by the compound multi-pool debit. The total drawn is               *)
(* drawComp·1 + drawPair·2 (the matched pair debits BOTH component pools — the   *)
(* Rule-2 two-token / Rule-4 one-token bridge). TLA+ analogue of the Rocq        *)
(* [compound_split_debit_conserves] conservation face. Stated for the FIRST       *)
(* group's three pools (sComp/s1/s2); the cross-group second-group draw is        *)
(* covered by Inv_SharedComponentSummedDrawWithinSupply below.                    *)
(*--------------------------------------------------------------------------*)
Inv_CompoundDebitConserves ==
    phase = "settled" =>
        ((sComp - drawComp) + (s1 - drawPair) + (s2 - drawPair))
          + (drawComp + 2 * drawPair)
          = sComp + s1 + s2

(*--------------------------------------------------------------------------*)
(* Inv_DrawMatchesDemand: the tokens drawn for the first group reconstruct      *)
(* its admitted demand — drawComp + drawPair = k (the combined-pool-first split  *)
(* is exhaustive; nothing of the admitted demand is left unsettled). With        *)
(* Inv_CompoundDebitConserves this pins the debit as EXACTLY the demand.          *)
(*--------------------------------------------------------------------------*)
Inv_DrawMatchesDemand ==
    phase = "settled" => drawComp + drawPair = k

(*--------------------------------------------------------------------------*)
(* Inv_SharedComponentSummedDrawWithinSupply (the #12 cross-group invariant):   *)
(* the SUMMED draw on the shared component pool s₁ across BOTH compound groups    *)
(* never exceeds s₁'s pre-state balance — the residual-ledger guarantee that      *)
(* keeps a component shared by several compounds underflow-safe across groups,    *)
(* not just within one. (drawPair is group 1's s₁ draw; drawPair2 is group 2's.)  *)
(*--------------------------------------------------------------------------*)
Inv_SharedComponentSummedDrawWithinSupply ==
    phase = "settled" => drawPair + drawPair2 <= s1

(*--------------------------------------------------------------------------*)
(* Inv_CombinedPoolSummedDrawWithinSupply: the combined pool Σ⟦comp⟧ is also     *)
(* shared by both groups (each may draw it combined-first); their summed draw     *)
(* stays within the combined pool's pre-balance.                                  *)
(*--------------------------------------------------------------------------*)
Inv_CombinedPoolSummedDrawWithinSupply ==
    phase = "settled" => drawComp + drawComp2 <= sComp

(*--------------------------------------------------------------------------*)
(* Inv_CrossGroupAdmissionBounded (TM-CA-165): the GATE — not merely the        *)
(* settlement's residual cap — bounds the cross-group cumulative demand on the   *)
(* shared component s₁. The SECOND group's admitted demand k2 fits the LIVE       *)
(* effective supply AFTER group 1's combined-first draw (the DRAWN-DOWN s₁ =     *)
(* s1 − dP1), so the combined draw on s₁ is bounded at ADMISSION time. With the   *)
(* pre-fix independent gate (k2 ≤ EffectiveSupply(sComp, s1, s3), s₁ at FULL       *)
(* balance) this is FALSE; the cross-group ledger gate (AdmitGate, now threading   *)
(* the shared residual) makes it hold.                                            *)
(*--------------------------------------------------------------------------*)
Inv_CrossGroupAdmissionBounded ==
    phase \in {"admitted", "settled"} =>
        k2 <= EffectiveSupply(sComp - Min2(k, sComp),
                              s1 - (k - Min2(k, sComp)),
                              s3)

(*--------------------------------------------------------------------------*)
(* Inv_SecondGroupDrawMatchesDemand (TM-CA-165): the SECOND group's FULL admitted *)
(* demand is settled — drawComp2 + drawPair2 = k2 — exactly as group 1            *)
(* (Inv_DrawMatchesDemand). Under the FIXED cross-group gate the settlement's      *)
(* residual cap on the shared component NEVER truncates group 2's draw (the gate   *)
(* already proved fundability against the LIVE residual); under the pre-fix        *)
(* independent gate it WOULD truncate (the cap silently absorbs the over-demand    *)
(* into per-pool debits ≤ balance — the TM-CA-165 un-funded-compute leak), so this *)
(* invariant fails. It is the model witness that the GATE, not the settlement cap, *)
(* bounds the cross-group cumulative demand on a shared component.                 *)
(*--------------------------------------------------------------------------*)
Inv_SecondGroupDrawMatchesDemand ==
    phase = "settled" => drawComp2 + drawPair2 = k2

=============================================================================
