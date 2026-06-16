--------------------------- MODULE TokenGatedJoin ----------------------------
(****************************************************************************)
(* MAJOR-5 — the token-gated-join sequential-fuel griefing / atomicity        *)
(* obligation (W1 red-team).                                                  *)
(*                                                                          *)
(* A token-gated multi-clause receive                                         *)
(*     for( {% y1<-x1 %}[s1] & {% y2<-x2 %}[s2] ){ P }                         *)
(* meters clause i against signer i's pool Σ⟦sᵢ⟧ (the demand Δᵢ). The W1       *)
(* red-team flagged that IF fuel is acquired at RUNTIME via nested receives,   *)
(* native bills a token at `eval_receive` ENTRY (before match), so an          *)
(* under-funded / partial join could PARK with a PREFIX of fuels SPENT —        *)
(* observable to a concurrent group — yielding a GRIEFING vector (a griefer    *)
(* funding k−1 of k clauses to drain a victim's pool without firing the join). *)
(*                                                                          *)
(* CRUX (settled from the f1r3node code — see the proof block below): the       *)
(* NATIVE runtime does NOT acquire fuel at runtime. A deploy's funding is        *)
(* settled ATOMICALLY at ACCEPTANCE by a PURE static per-pool analysis:          *)
(*   - rholang/.../accounting/delta_sigma.rs::demand computes Δ_s as a linear-   *)
(*     time static COMM-count BEFORE any speculative execution; is_funded is a    *)
(*     single all-or-nothing inequality effectiveΣ_s ≥ Δ_s.                        *)
(*   - casper/.../util/rholang/acceptance.rs::admit_by_funding reads each pool     *)
(*     Σ⟦s⟧ from the consensus pre-state and admits a group ONLY if funded; a      *)
(*     REJECTED deploy produces NO SettlementDebit (tests assert                   *)
(*     `outcome.debits.is_empty()` on reject — "no tokens consumed").              *)
(*   - the on-chain debit Σ⟦s⟧ -= ΣΔ_admitted is computed by                       *)
(*     compute_settlement_debits AFTER the admission walk and applied at           *)
(*     settlement — atomically, only for admitted deploys.                          *)
(*   - reduce.rs `reserve_comm` at eval_send/eval_receive charges an IN-MEMORY      *)
(*     per-deploy Budget, NOT Σ⟦s⟧; and under D3/DR-9 accepted deploys run          *)
(*     UNMETERED-for-liveness, so the runtime budget never gates an admitted        *)
(*     deploy's COMMs. No runtime partial-fuel state on Σ⟦s⟧ ever exists.            *)
(*                                                                          *)
(* So the griefing vector is an artifact of the TRANSPILER's runtime-gate model    *)
(* (nested receives), NOT a hazard of the native acceptance-time model. This spec   *)
(* models BOTH regimes to make that difference machine-checkable.                   *)
(*                                                                          *)
(* ── SETUP (decoupled pools so the property witnesses are clean) ───────────────  *)
(* THREE concurrent groups contend within one block, over FOUR signer pools:        *)
(*   • PRINCIPAL — a two-clause token-gated join over its OWN pools s1, s2          *)
(*       (the join under test). Demands D1, D2.                                      *)
(*   • VICTIM    — a legitimate single-clause join over pool sV. Demand DV.          *)
(*   • GRIEFER   — a two-clause join whose clauses draw its OWN pool sG AND the       *)
(*       VICTIM's pool sV. The attack: fund the sV-side clause but WITHHOLD the       *)
(*       sG-side (k−1 of k), trying to drain sV without ever firing the join.          *)
(* The principal's pools are DISJOINT from the griefer's (no spurious coupling); the   *)
(* ONLY contended pool is sV (griefer-clause-2 vs the victim group) — exactly the       *)
(* drain target the red-team described.                                                 *)
(*                                                                          *)
(*   M1 (NATIVE, acceptance-time)   — a single atomic admit-or-reject step PER group.  *)
(*       A group fires iff EVERY clause's pool clears its demand AND its data is        *)
(*       present; then ALL its pools are debited simultaneously. Otherwise REJECT with   *)
(*       its pools COMPLETELY UNCHANGED (no prefix spent). This is admit_by_funding's    *)
(*       reject-both + the atomic compute_settlement_debits.                              *)
(*   M2 (TRANSPILER, runtime sequential gates) — each clause acquired one at a time in    *)
(*       a NONDETERMINISTIC interleaving; each acquisition debits its pool at gate ENTRY   *)
(*       (mirroring `reserve_comm` at eval_receive entry, BEFORE match). A join PARKS if   *)
(*       a clause cannot acquire (pool empty) or its data is absent — leaving a PREFIX of   *)
(*       pools debited. This is the CompoundProtocol.tla nested-gate shape.                 *)
(*                                                                          *)
(* Both regimes run on SEPARATE copies of the four pools from the SAME pre-state, so the    *)
(* funded-path equivalence (P1) compares M1's terminal pools to M2's terminal pools over     *)
(* identical inputs.                                                                          *)
(*                                                                          *)
(* Properties (see the INVARIANTS section):                                                  *)
(*   P1  Inv_FundedPathEquivalence        — funded path: M1.final == M2.final (order-indep).   *)
(*   P2a Inv_M1_AtomicNoPartialPrefix     — M1: a group's pools are EITHER all debited (fire)   *)
(*                                          OR all untouched (reject) — never a strict prefix.   *)
(*   P2b Inv_M1_NoVictimDrainWithoutFire  — M1: sV is debited ONLY by a group that FIRED        *)
(*                                          (no griefing). THE NATIVE REFUTATION OF MAJOR-5.     *)
(*   P2c Inv_M2_NoVictimDrainWithoutFire  — M2: SAME claim; TLC produces a COUNTEREXAMPLE,       *)
(*                                          confirming the vector is real for the transpiler.    *)
(*   P3a Inv_NoUnderflow                  — no pool, either track, ever goes < 0.                *)
(*   P3b Inv_NoCrossSignerTheft           — each pool is only ever DEBITED (never credited by a   *)
(*                                          foreign lane), and by at most the demands drawing it. *)
(*   P3c Inv_Conservation                 — native: remaining pools + consumed = pre-state.       *)
(*   P4  Inv_ConservationOfAuthority      — grouping (compound vs separate) never changes the      *)
(*                                          per-signer total: principal draws EXACTLY D1 from s1    *)
(*                                          and D2 from s2 on fire.                                  *)
(*                                                                          *)
(* Style mirrors CompoundSettlement.tla / CompoundProtocol.tla (small focused module; bounded-      *)
(* exhaustive over pre-state balances 0..MaxSupply; Inv_-prefixed invariants; paired MC wrapper      *)
(* + .cfg).                                                                                            *)
(****************************************************************************)

EXTENDS Integers, FiniteSets

CONSTANTS
    MaxSupply       \* Nat: each signer pool ranges over 0..MaxSupply
                    \* (kept small per the bounded-memory envelope: 0..3).

ASSUME MaxSupply \in Nat

Min2(a, b) == IF a <= b THEN a ELSE b

(*--------------------------------------------------------------------------*)
(* MODEL PARAMETERS (fixed, not swept — the JOIN SHAPE).                       *)
(*                                                                          *)
(* Per-clause demands: each clause is one rendezvous COMM (the join-sequential- *)
(* fuel rule's "one fuel unit per atom", w1 doc §4), so Δ = 1 per clause and the *)
(* funded boundary is exactly Σ⟦sᵢ⟧ ≥ 1.                                        *)
(*--------------------------------------------------------------------------*)
D1 == 1     \* Δ for principal clause 1 (pool s1)
D2 == 1     \* Δ for principal clause 2 (pool s2)
DV == 1     \* Δ for the victim's single clause (pool sV)
DGsg == 1   \* Δ for the griefer's sG-side clause (pool sG)
DGsv == 1   \* Δ for the griefer's sV-side clause (pool sV) — the drain unit

(*--------------------------------------------------------------------------*)
(* State.                                                                    *)
(*                                                                          *)
(* PRE-state pool balances (fixed at Init, nondeterministic over 0..MaxSupply):  *)
(*   s1, s2 : the principal join's two clause pools Σ⟦s1⟧, Σ⟦s2⟧ (DISJOINT from   *)
(*            every other group).                                                  *)
(*   sV     : the VICTIM's pool — ALSO the griefer's clause-2 (drain) pool.        *)
(*   sG     : the griefer's OWN clause-1 pool (the side the attack WITHHOLDS).      *)
(*                                                                          *)
(* Data-presence flags (fixed at Init, over BOOLEAN): a clause's COMM can only     *)
(* commit if its data message is present; the M2 gate is charged at ENTRY before    *)
(* the match, so a present-fuel / absent-data clause still spends under M2.          *)
(*   dataP1, dataP2 : principal clauses' data on x1, x2.                            *)
(*   dataV          : victim clause's data.                                         *)
(*   dataGsg        : griefer's sG-side data (WITHHELD in the canonical attack).     *)
(*                                                                          *)
(* M1 (native) track — atomic per-group admit/reject, applied in one step:          *)
(*   m1Phase    : "pregate" → "done".                                              *)
(*   m1S1,m1S2  : principal pools after M1.   m1Fired      : principal fired?        *)
(*   m1V        : victim pool after M1.        m1VFired     : victim group fired?      *)
(*   m1SG       : griefer sG pool after M1.    m1GriefFired : griefer group fired?      *)
(*                                                                          *)
(* M2 (transpiler) track — runtime sequential gates, each charged at entry:           *)
(*   m2S1,m2S2  : principal pools.  m2Got1,m2Got2 : principal clause gates fired.       *)
(*   m2Fired    : principal COMM body fired (both gates + data).                        *)
(*   m2V        : victim pool.       m2VGot : victim clause gate fired.  m2VFired.        *)
(*   m2SG       : griefer sG pool.   m2GriefGotSG, m2GriefGotSV : griefer clause gates.   *)
(*   m2GriefFired : griefer COMM body fired.                                             *)
(*--------------------------------------------------------------------------*)
VARIABLES
    \* Shared pre-state (fixed at Init).
    s1, s2, sV, sG,
    dataP1, dataP2, dataV, dataGsg,
    \* M1 (native) track.
    m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
    \* M2 (transpiler) track.
    m2S1, m2S2, m2V, m2SG,
    m2Got1, m2Got2, m2Fired,
    m2VGot, m2VFired,
    m2GriefGotSG, m2GriefGotSV, m2GriefFired

vars == <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
          m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
          m2S1, m2S2, m2V, m2SG, m2Got1, m2Got2, m2Fired,
          m2VGot, m2VFired, m2GriefGotSG, m2GriefGotSV, m2GriefFired>>

Supplies == 0..MaxSupply

(*--------------------------------------------------------------------------*)
(* TYPE INVARIANT.                                                            *)
(*--------------------------------------------------------------------------*)
TypeOK ==
    /\ s1 \in Supplies /\ s2 \in Supplies /\ sV \in Supplies /\ sG \in Supplies
    /\ dataP1 \in BOOLEAN /\ dataP2 \in BOOLEAN
    /\ dataV  \in BOOLEAN /\ dataGsg \in BOOLEAN
    /\ m1Phase \in {"pregate", "done"}
    /\ m1S1 \in Supplies /\ m1S2 \in Supplies /\ m1V \in Supplies /\ m1SG \in Supplies
    /\ m1Fired \in BOOLEAN /\ m1VFired \in BOOLEAN /\ m1GriefFired \in BOOLEAN
    /\ m2S1 \in Supplies /\ m2S2 \in Supplies /\ m2V \in Supplies /\ m2SG \in Supplies
    /\ m2Got1 \in BOOLEAN /\ m2Got2 \in BOOLEAN /\ m2Fired \in BOOLEAN
    /\ m2VGot \in BOOLEAN /\ m2VFired \in BOOLEAN
    /\ m2GriefGotSG \in BOOLEAN /\ m2GriefGotSV \in BOOLEAN /\ m2GriefFired \in BOOLEAN

(*--------------------------------------------------------------------------*)
(* INITIAL STATE.                                                             *)
(*                                                                          *)
(* PRE-state pool balances range over EVERY combination in 0..MaxSupply; data-  *)
(* presence flags over EVERY combination of BOOLEAN. Both regime tracks start    *)
(* seeded to the pre-state (no debit yet).                                       *)
(*--------------------------------------------------------------------------*)
Init ==
    /\ s1 \in Supplies /\ s2 \in Supplies /\ sV \in Supplies /\ sG \in Supplies
    /\ dataP1 \in BOOLEAN /\ dataP2 \in BOOLEAN
    /\ dataV  \in BOOLEAN /\ dataGsg \in BOOLEAN
    \* M1 track seeded to the pre-state.
    /\ m1Phase = "pregate"
    /\ m1S1 = s1 /\ m1S2 = s2 /\ m1V = sV /\ m1SG = sG
    /\ m1Fired = FALSE /\ m1VFired = FALSE /\ m1GriefFired = FALSE
    \* M2 track seeded to the pre-state.
    /\ m2S1 = s1 /\ m2S2 = s2 /\ m2V = sV /\ m2SG = sG
    /\ m2Got1 = FALSE /\ m2Got2 = FALSE /\ m2Fired = FALSE
    /\ m2VGot = FALSE /\ m2VFired = FALSE
    /\ m2GriefGotSG = FALSE /\ m2GriefGotSV = FALSE /\ m2GriefFired = FALSE

(*==========================================================================*)
(* M1 — NATIVE (acceptance-time, all-or-nothing, per group).                 *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* Per-group FUNDABILITY: the per-pool funding test Σ⟦sᵢ⟧ ≥ Δᵢ for ALL of a     *)
(* group's clauses (delta_sigma::is_funded applied per clause; admit_by_funding  *)
(* admits the group iff EVERY clause's pool clears its demand). Funding is        *)
(* STATIC — it does NOT depend on data presence.                                 *)
(*--------------------------------------------------------------------------*)
PrincipalFunded == (s1 >= D1) /\ (s2 >= D2)
VictimFunded    == (sV >= DV)
GriefFunded     == (sG >= DGsg) /\ (sV >= DGsv)    \* needs BOTH sides — the attack withholds sG

(*--------------------------------------------------------------------------*)
(* A group FIRES under M1 iff funded AND ALL its data is present (the COMM can    *)
(* only commit with its messages). On a NON-fire the group is rejected with NO    *)
(* debit; on a fire ALL its pools are debited atomically.                          *)
(*--------------------------------------------------------------------------*)
PrincipalFiresM1 == PrincipalFunded /\ dataP1 /\ dataP2
VictimFiresM1    == VictimFunded /\ dataV
GriefFiresM1     == GriefFunded /\ dataGsg            \* sV-side data implicit (the drain is unconditional in M2, but M1 fires only if the WHOLE group commits)

(*--------------------------------------------------------------------------*)
(* M1 admit/settle — a SINGLE atomic step deciding ALL three groups. Each group  *)
(* is admitted all-or-nothing; the contended pool sV is drawn by the victim group *)
(* (DV on its fire) AND the griefer group (DGsv on its fire). The griefer fires    *)
(* ONLY if it funds BOTH sides — so the "fund only k−1" attack (withhold sG, i.e.   *)
(* sG < DGsg OR dataGsg = FALSE) makes GriefFiresM1 FALSE ⇒ sV is NOT drained by     *)
(* the griefer. THAT is the native no-griefing guarantee.                            *)
(*                                                                          *)
(* Pool writes are applied in one step; the sV pool is debited DV (victim fire) +    *)
(* DGsv (griefer fire), each conditional on that group's fire. Since native admits    *)
(* both only if their respective pools clear demand AND the model sweeps all pre-      *)
(* states, sV could be drawn by both — but the admission ensured sV ≥ DV and sV ≥ DGsv  *)
(* PER GROUP; the SUMMED draw may exceed sV. The native gate's cross-group residual     *)
(* ledger (compute_settlement_debits) prevents over-draw: we model it by drawing the     *)
(* victim FIRST, then the griefer against the LIVE residual (sV − victim draw), exactly   *)
(* as the Rust ledger bounds shared-pool draws (CompoundSettlement residual ledger).      *)
(*--------------------------------------------------------------------------*)
AdmitM1 ==
    /\ m1Phase = "pregate"
    /\ LET pf == PrincipalFiresM1
           vf == VictimFiresM1
           \* sV residual ledger: the victim group (legitimate) draws sV FIRST; the
           \* griefer is then admitted on the shared sV pool ONLY against the LIVE
           \* residual (compute_settlement_debits' cross-group ledger). The griefer
           \* "fires" — and ATOMICALLY debits BOTH its sG side AND its sV side —
           \* iff it is funded on BOTH sides, its data is present, AND the residual
           \* sV (after the victim) still covers its full DGsv draw. This is the
           \* native all-or-nothing admit on the joint block settlement: a griefer
           \* that cannot secure its full sV unit is rejected (no sG draw, no sV draw),
           \* exactly as the gate rejects a group it cannot fully fund.
           vDraw  == IF vf THEN DV ELSE 0
           sVafterVictim == sV - vDraw
           gf == GriefFiresM1 /\ (sVafterVictim >= DGsv)
           gDraw  == IF gf THEN DGsv ELSE 0
       IN  /\ m1S1' = IF pf THEN m1S1 - D1 ELSE m1S1
           /\ m1S2' = IF pf THEN m1S2 - D2 ELSE m1S2
           /\ m1SG' = IF gf THEN m1SG - DGsg ELSE m1SG   \* atomic with the sV draw
           /\ m1V'  = sV - vDraw - gDraw
           /\ m1Fired' = pf
           /\ m1VFired' = vf
           /\ m1GriefFired' = gf
    /\ m1Phase' = "done"
    /\ UNCHANGED <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
                   m2S1, m2S2, m2V, m2SG, m2Got1, m2Got2, m2Fired,
                   m2VGot, m2VFired, m2GriefGotSG, m2GriefGotSV, m2GriefFired>>

(*==========================================================================*)
(* M2 — TRANSPILER (runtime sequential nested gates), EACH charged at entry.  *)
(*==========================================================================*)

\* Principal clause 1 gate (draws m2S1) — charged at entry, regardless of clause 2.
M2_AcquireClause1 ==
    /\ ~m2Got1 /\ ~m2Fired
    /\ m2S1 >= D1
    /\ m2S1' = m2S1 - D1
    /\ m2Got1' = TRUE
    /\ UNCHANGED <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
                   m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
                   m2S2, m2V, m2SG, m2Got2, m2Fired,
                   m2VGot, m2VFired, m2GriefGotSG, m2GriefGotSV, m2GriefFired>>

\* Principal clause 2 gate (draws m2S2) — fires in EITHER order vs clause 1.
M2_AcquireClause2 ==
    /\ ~m2Got2 /\ ~m2Fired
    /\ m2S2 >= D2
    /\ m2S2' = m2S2 - D2
    /\ m2Got2' = TRUE
    /\ UNCHANGED <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
                   m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
                   m2S1, m2V, m2SG, m2Got1, m2Fired,
                   m2VGot, m2VFired, m2GriefGotSG, m2GriefGotSV, m2GriefFired>>

\* Principal COMM body fires: both gates acquired + both data present (fuel already spent).
M2_PrincipalFire ==
    /\ m2Got1 /\ m2Got2 /\ ~m2Fired
    /\ dataP1 /\ dataP2
    /\ m2Fired' = TRUE
    /\ UNCHANGED <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
                   m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
                   m2S1, m2S2, m2V, m2SG, m2Got1, m2Got2,
                   m2VGot, m2VFired, m2GriefGotSG, m2GriefGotSV, m2GriefFired>>

\* Victim single-clause gate (draws m2V) — charged at entry.
M2_VictimAcquire ==
    /\ ~m2VGot /\ ~m2VFired
    /\ m2V >= DV
    /\ m2V' = m2V - DV
    /\ m2VGot' = TRUE
    /\ UNCHANGED <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
                   m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
                   m2S1, m2S2, m2SG, m2Got1, m2Got2, m2Fired,
                   m2VFired, m2GriefGotSG, m2GriefGotSV, m2GriefFired>>

\* Victim COMM fires: its gate acquired + its data present.
M2_VictimFire ==
    /\ m2VGot /\ ~m2VFired
    /\ dataV
    /\ m2VFired' = TRUE
    /\ UNCHANGED <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
                   m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
                   m2S1, m2S2, m2V, m2SG, m2Got1, m2Got2, m2Fired,
                   m2VGot, m2GriefGotSG, m2GriefGotSV, m2GriefFired>>

(*--------------------------------------------------------------------------*)
(* GRIEFER under M2 — the EXPLOIT path. The griefer's sV-side clause draws the    *)
(* VICTIM pool m2V at gate ENTRY. It can acquire that gate (draining sV) WITHOUT   *)
(* ever acquiring its sG-side — funds only k−1 of k. The join then PARKS (never    *)
(* fires), but sV has been debited. THIS is the drain-without-fire the red-team     *)
(* flagged.                                                                          *)
(*--------------------------------------------------------------------------*)
\* griefer sV-side gate (draws the VICTIM pool m2V) — charged at entry, no sG needed.
M2_GriefAcquireSV ==
    /\ ~m2GriefGotSV /\ ~m2GriefFired
    /\ m2V >= DGsv
    /\ m2V' = m2V - DGsv          \* victim pool drained at the griefer's gate ENTRY
    /\ m2GriefGotSV' = TRUE
    /\ UNCHANGED <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
                   m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
                   m2S1, m2S2, m2SG, m2Got1, m2Got2, m2Fired,
                   m2VGot, m2VFired, m2GriefGotSG, m2GriefFired>>

\* griefer sG-side gate (draws m2SG) — the side WITHHELD in the canonical attack.
\* TLC still lets it fire (so the funded-griefer case is explored); the exploit
\* trace is the one where this NEVER fires but M2_GriefAcquireSV did.
M2_GriefAcquireSG ==
    /\ ~m2GriefGotSG /\ ~m2GriefFired
    /\ m2SG >= DGsg
    /\ m2SG' = m2SG - DGsg
    /\ m2GriefGotSG' = TRUE
    /\ UNCHANGED <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
                   m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
                   m2S1, m2S2, m2V, m2Got1, m2Got2, m2Fired,
                   m2VGot, m2VFired, m2GriefGotSV, m2GriefFired>>

\* griefer COMM fires iff BOTH gates acquired AND its sG-side data present.
\* In the EXPLOIT it never fires (sG withheld), yet sV was already drained.
M2_GriefFire ==
    /\ m2GriefGotSG /\ m2GriefGotSV /\ ~m2GriefFired
    /\ dataGsg
    /\ m2GriefFired' = TRUE
    /\ UNCHANGED <<s1, s2, sV, sG, dataP1, dataP2, dataV, dataGsg,
                   m1Phase, m1S1, m1S2, m1V, m1SG, m1Fired, m1VFired, m1GriefFired,
                   m2S1, m2S2, m2V, m2SG, m2Got1, m2Got2, m2Fired,
                   m2VGot, m2VFired, m2GriefGotSG, m2GriefGotSV>>

(*--------------------------------------------------------------------------*)
(* NEXT.                                                                       *)
(*--------------------------------------------------------------------------*)
Next ==
    \/ AdmitM1
    \/ M2_AcquireClause1
    \/ M2_AcquireClause2
    \/ M2_PrincipalFire
    \/ M2_VictimAcquire
    \/ M2_VictimFire
    \/ M2_GriefAcquireSV
    \/ M2_GriefAcquireSG
    \/ M2_GriefFire

Spec == Init /\ [][Next]_vars

(*--------------------------------------------------------------------------*)
(* Terminal predicate: both regimes have run to quiescence. M1 is terminal     *)
(* once m1Phase = "done"; M2 is terminal when no M2 action is enabled (every     *)
(* gate acquired or its pool exhausted; every body fired iff it can).            *)
(*--------------------------------------------------------------------------*)
M2_Quiescent ==
    /\ (m2Got1 \/ m2S1 < D1)
    /\ (m2Got2 \/ m2S2 < D2)
    /\ (m2Fired \/ ~(m2Got1 /\ m2Got2 /\ dataP1 /\ dataP2))
    /\ (m2VGot \/ m2V < DV)
    /\ (m2VFired \/ ~(m2VGot /\ dataV))
    /\ (m2GriefGotSV \/ m2V < DGsv)
    /\ (m2GriefGotSG \/ m2SG < DGsg)
    /\ (m2GriefFired \/ ~(m2GriefGotSG /\ m2GriefGotSV /\ dataGsg))

Terminal == (m1Phase = "done") /\ M2_Quiescent

(*==========================================================================*)
(* INVARIANTS / PROPERTIES                                                    *)
(*==========================================================================*)

(*--------------------------------------------------------------------------*)
(* P2a — Inv_M1_AtomicNoPartialPrefix (the "no partial-fuel state" guarantee):   *)
(* under the NATIVE model, each group's clause-pools are EITHER all debited       *)
(* (the group fired) OR all untouched (the group was rejected) — there is NEVER   *)
(* a state in which a STRICT, non-empty prefix of a group's pools is debited.      *)
(* For the principal join (its pools s1, s2 are disjoint from all other groups,    *)
(* so they are touched ONLY by the principal's own fire):                          *)
(*   fired  ⇒ m1S1 = s1 − D1 ∧ m1S2 = s2 − D2   (all debited)                       *)
(*   ~fired ⇒ m1S1 = s1      ∧ m1S2 = s2        (none debited)                       *)
(* This is the formal content of "no runtime partial-fuel state on Σ⟦s⟧ exists".    *)
(*--------------------------------------------------------------------------*)
Inv_M1_AtomicNoPartialPrefix ==
    (m1Phase = "done") =>
        /\ (m1Fired  => (m1S1 = s1 - D1 /\ m1S2 = s2 - D2))
        /\ (~m1Fired => (m1S1 = s1      /\ m1S2 = s2))

(*--------------------------------------------------------------------------*)
(* P2b — Inv_M1_NoVictimDrainWithoutFire (the native NO-GRIEFING guarantee):      *)
(* under M1 the victim pool sV is debited ONLY by a group that actually FIRED      *)
(* (the victim's own legitimate join, OR a griefer that fully funded BOTH sides     *)
(* and fired). There is NO state in which sV is drained while the draining group     *)
(* is parked/rejected. In particular a griefer funding only k−1 of k (withholding     *)
(* sG) does NOT fire ⇒ does NOT draw sV. THIS is the formal refutation of the         *)
(* MAJOR-5 griefing vector FOR THE NATIVE MODEL.                                       *)
(*                                                                          *)
(* Stated as: the TOTAL drain on sV equals (victim drew DV iff it fired) + (griefer    *)
(* drew DGsv iff IT fired); equivalently, any drain beyond the victim's own fire        *)
(* implies the griefer fired.                                                            *)
(*--------------------------------------------------------------------------*)
Inv_M1_NoVictimDrainWithoutFire ==
    (m1Phase = "done") =>
        \* sV decreases ONLY by groups that fired; no fire ⇒ no drain.
        /\ ((~m1VFired /\ ~m1GriefFired) => (m1V = sV))
        \* and the drain is bounded by exactly the firing groups' demands.
        /\ (m1V = sV - (IF m1VFired THEN DV ELSE 0) - (IF m1GriefFired THEN DGsv ELSE 0))

(*--------------------------------------------------------------------------*)
(* P2c — Inv_M2_NoVictimDrainWithoutFire: the SAME claim asserted for the          *)
(* TRANSPILER model. Under M2 the griefer debits sV at its sV-side gate ENTRY        *)
(* without ever firing (sG withheld). TLC produces a COUNTEREXAMPLE — a reachable     *)
(* terminal state with m2V < sV, ~m2GriefFired, AND ~m2VFired (so the drain is NOT     *)
(* the victim's own legitimate fire) — CONFIRMING the griefing vector is REAL for      *)
(* the transpiler's runtime-gate model. Checked in the companion                       *)
(* TokenGatedJoinM2Grief.cfg; kept OUT of the native suite's INVARIANTS list so the      *)
(* native suite stays green.                                                             *)
(*--------------------------------------------------------------------------*)
Inv_M2_NoVictimDrainWithoutFire ==
    Terminal =>
        ((m2V < sV /\ ~m2VFired) => m2GriefFired)

(*--------------------------------------------------------------------------*)
(* P2c (focused) — Inv_M2_NoGrieferDrainOfVictim: isolates the GRIEFING face of   *)
(* the M2 partial-fuel hazard from the victim's own data-absent self-park. It      *)
(* asserts that the GRIEFER never spends the victim's token (acquires its sV-side   *)
(* gate) unless its whole join fires. TLC REFUTES this with a counterexample in     *)
(* which m2GriefGotSV = TRUE (the griefer drew DGsv from the victim pool at gate     *)
(* ENTRY) but m2GriefFired = FALSE (the sG side was withheld / its data absent) —    *)
(* the EXACT "fund k−1 of k to drain a victim without firing" exploit the red-team   *)
(* described. Pair with TokenGatedJoinM2Grief.cfg. The contrast with the native      *)
(* Inv_M1_NoVictimDrainWithoutFire (which HOLDS) is the bottom line: the vector is    *)
(* an artifact of the runtime-gate model, not the acceptance-time model.              *)
(*--------------------------------------------------------------------------*)
Inv_M2_NoGrieferDrainOfVictim ==
    Terminal =>
        (m2GriefGotSV => m2GriefFired)

(*--------------------------------------------------------------------------*)
(* P1 — Inv_FundedPathEquivalence: on the FULLY-FUNDED principal path (both        *)
(* principal pools clear demand AND both data present ⇒ the principal join fires     *)
(* in BOTH regimes), the two regimes reach the SAME terminal principal-pool state:    *)
(* the M2 gate ORDER does not change the outcome (commutative ∘), and it equals the    *)
(* M1 atomic outcome. The principal pools are disjoint from the griefer, so this is    *)
(* an unconditional per-group equality (no need to fence off griefer interference).     *)
(* This is the funded-path trace-equivalence the w1 doc states.                          *)
(*--------------------------------------------------------------------------*)
PrincipalFullyFunded == (s1 >= D1) /\ (s2 >= D2) /\ dataP1 /\ dataP2

Inv_FundedPathEquivalence ==
    (Terminal /\ PrincipalFullyFunded) =>
        /\ m1Fired = TRUE
        /\ m2Fired = TRUE
        /\ m1S1 = m2S1
        /\ m1S2 = m2S2

(*--------------------------------------------------------------------------*)
(* P3a — Inv_NoUnderflow: NO pool on EITHER track is ever debited below zero.       *)
(* The native gate admits only funded groups (Σ ≥ Δ) with a residual ledger on the   *)
(* shared sV; each M2 gate is guarded by `pool ≥ Δ`. Mirrors                          *)
(* CompoundSettlement.Inv_ComponentDrawNoUnderflow.                                    *)
(*--------------------------------------------------------------------------*)
Inv_NoUnderflow ==
    /\ m1S1 >= 0 /\ m1S2 >= 0 /\ m1V >= 0 /\ m1SG >= 0
    /\ m2S1 >= 0 /\ m2S2 >= 0 /\ m2V >= 0 /\ m2SG >= 0

(*--------------------------------------------------------------------------*)
(* P3b — Inv_NoCrossSignerTheft: each pool is only ever DEBITED (never credited by   *)
(* a foreign lane), and by at most the demands that draw it. The principal pools s1,  *)
(* s2 are drawn ONLY by the principal join (≤ D1, D2); the griefer's sG only by the    *)
(* griefer (≤ DGsg); the shared sV only by the victim (DV) and the griefer (DGsv),      *)
(* summed draw ≤ sV. No clause ever increases a pool. Stated on BOTH tracks.            *)
(*--------------------------------------------------------------------------*)
Inv_NoCrossSignerTheft ==
    \* M1: monotone non-increasing pools; principal/griefer-own pools bounded by own Δ.
    /\ m1S1 <= s1 /\ m1S1 >= s1 - D1
    /\ m1S2 <= s2 /\ m1S2 >= s2 - D2
    /\ m1SG <= sG /\ m1SG >= sG - DGsg
    /\ m1V  <= sV /\ m1V  >= sV - (DV + DGsv)        \* sV drawn by at most victim + griefer
    \* M2: same monotonicity (no lane credits a foreign pool).
    /\ m2S1 <= s1 /\ m2S1 >= s1 - D1
    /\ m2S2 <= s2 /\ m2S2 >= s2 - D2
    /\ m2SG <= sG /\ m2SG >= sG - DGsg
    /\ m2V  <= sV /\ m2V  >= sV - (DV + DGsv)

(*--------------------------------------------------------------------------*)
(* P3c — Inv_Conservation (native): remaining pools + tokens consumed = pre-state    *)
(* totals, per pool. The principal consumes D1 (s1) + D2 (s2) on fire; the victim     *)
(* DV (sV) on fire; the griefer DGsg (sG) + DGsv (sV) on fire. Mirrors                 *)
(* CompoundSettlement.Inv_CompoundDebitConserves.                                       *)
(*--------------------------------------------------------------------------*)
Inv_Conservation ==
    (m1Phase = "done") =>
        /\ m1S1 + (IF m1Fired THEN D1 ELSE 0) = s1
        /\ m1S2 + (IF m1Fired THEN D2 ELSE 0) = s2
        /\ m1SG + (IF m1GriefFired THEN DGsg ELSE 0) = sG
        /\ m1V  + (IF m1VFired THEN DV ELSE 0) + (IF m1GriefFired THEN DGsv ELSE 0) = sV

(*--------------------------------------------------------------------------*)
(* P4 — Inv_ConservationOfAuthority: GROUPING never changes the total a signer       *)
(* pays. The principal multi-clause (compound) join over {s1, s2} debits signer s1    *)
(* EXACTLY D1 and signer s2 EXACTLY D2 on its fire — the SAME totals as two SEPARATE   *)
(* single-clause joins each drawing its own pool its own Δ. Neither MORE (no double-    *)
(* charge for grouping) nor LESS (no grouping discount). Since s1, s2 are the           *)
(* principal's alone, (s1 − m1S1) and (s2 − m1S2) ARE the per-signer charges. Mirrors    *)
(* the P8 balanced-multi-sig "grouping is commutative ∘" claim.                           *)
(*--------------------------------------------------------------------------*)
Inv_ConservationOfAuthority ==
    (m1Phase = "done") =>
        /\ (m1Fired  => ((s1 - m1S1) = D1 /\ (s2 - m1S2) = D2))   \* fired ⇒ exactly Δ each
        /\ (~m1Fired => ((s1 - m1S1) = 0  /\ (s2 - m1S2) = 0))    \* rejected ⇒ zero each
        \* And the SAME on the M2 track on the funded path (compound = sequential = same per-signer total).
        /\ (PrincipalFullyFunded /\ Terminal
              => ((s1 - m2S1) = D1 /\ (s2 - m2S2) = D2))

=============================================================================
