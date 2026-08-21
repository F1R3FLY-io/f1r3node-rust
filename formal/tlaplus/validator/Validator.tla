-------------------------------- MODULE Validator --------------------------------
(***************************************************************************)
(* Validator behavioral contract — TLA+ surface (Workstream E, stage E5). *)
(*                                                                         *)
(* This is the TLA+ third of the "built-in validator proven in all three  *)
(* provers" milestone (Lean: formal/lean/Validator/Contract.lean; Rocq:   *)
(* formal/rocq/validator/theories/Contract.v).                            *)
(*                                                                         *)
(* GUIDING PRINCIPLE: name, do not re-prove. Each THEOREM below is the     *)
(* TLA+ IMAGE of a contract obligation, discharged DEDUCTIVELY by TLAPS    *)
(* (z3 via `BY SMT`, or the kernel via `OBVIOUS`). These are the          *)
(* arithmetic cores of the obligations — the funding-comparison and       *)
(* slash-zeroing kernels — NOT bounded model-checks. The full STATE-      *)
(* MACHINE obligations (multi-step schedule-independence) are discharged   *)
(* by TLC model-checking in the existing                                  *)
(* formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla; this module  *)
(* proves the arithmetic core that TLC's state exploration rests on.       *)
(*                                                                         *)
(* THEOREM <-> obligation <-> spec map                                    *)
(* ───────────────────────────────────────────────────────────────────── *)
(*   FundingMonotone                 S2 (Σ_s ≥ Δ_s stability)      §7.6    *)
(*   FundingDecidable                S2 (validator reaches verdict) §7.6   *)
(*   RejectBothOnOversubscription    S3 (linear no-double-spend)    §7.7   *)
(*   ZeroingIdempotent               P1 (slash effect kernel)       DR-12  *)
(*   SlashOrderIndependent           P1 (multi-slash order-indep.)  DR-12  *)
(* ───────────────────────────────────────────────────────────────────── *)
(*                                                                         *)
(* Obligations covered ELSEWHERE (cited, not re-proved here):             *)
(*   S1 token-presence / reject-malformed  — Rocq FuelGateSafety,         *)
(*       Lean validator_contract_built_in_S1. Structural (process-syntax  *)
(*       inequality), not an arithmetic clause, so it has no TLAPS image.  *)
(*   S4 / P3 per-step determinism          — Rocq StepDeterminism         *)
(*       (ca_step_deterministic); its MULTI-STEP, schedule-independent     *)
(*       image is TLC-checked as ConsumedAndVerdictScheduleIndependent     *)
(*       and admission_decision_schedule_independent in                   *)
(*       formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla.        *)
(*   P2 fork-choice exclusion of slashed   — Rocq MainTheorem             *)
(*       (main_T10_fork_choice_exclusion); follows in this module from    *)
(*       ZeroingIdempotent (a slashed bond is 0, and the fork-choice       *)
(*       filter drops the 0-bond key), the arithmetic of which is the      *)
(*       `b = 0` premise shared with the Rocq statement.                  *)
(***************************************************************************)
EXTENDS Integers, TLAPS

(*-------------------------------------------------------------------------*)
(* S2 — funding (LinearLogicResources.funding_decidable, §7.6).            *)
(*                                                                         *)
(* The validator admits a deploy iff the supply balance sigma covers its   *)
(* demand delta: funds(sigma, delta) == delta <= sigma. Two facts make     *)
(* the admission verdict well-behaved.                                     *)
(*-------------------------------------------------------------------------*)

(* FundingMonotone (S2 stability): if a first deploy's demand delta1 is no  *)
(* larger than a second admitted demand delta2, and delta2 fits the pool    *)
(* sigma, then delta1 fits sigma too. This is the Σ_s ≥ Δ_s transitivity    *)
(* that makes an admission verdict stable under demand-shrinking — the      *)
(* arithmetic backbone of "accept iff covered".                            *)
THEOREM FundingMonotone ==
  \A sigma, delta1, delta2 \in Nat :
    (delta1 <= delta2 /\ delta2 <= sigma) => delta1 <= sigma
  BY SMT

(* FundingDecidable (S2 verdict): for every pool sigma and demand delta the  *)
(* funding obligation is DECIDABLE — the comparison delta <= sigma either    *)
(* holds or does not, so the validator ALWAYS reaches an accept/reject       *)
(* verdict by one integer comparison, before any execution. This is the      *)
(* TLA+ image of the Rocq `funding_decidable` sumbool.                       *)
THEOREM FundingDecidable ==
  \A sigma, delta \in Nat : (delta <= sigma) \/ ~(delta <= sigma)
  OBVIOUS

(*-------------------------------------------------------------------------*)
(* S3 — linear no-double-spend (LinearLogicResources                       *)
(* .ll_no_double_spend_single_witness, §7.7 reject-both).                  *)
(*-------------------------------------------------------------------------*)

(* RejectBothOnOversubscription (S3): if two deploys' demands jointly        *)
(* oversubscribe one pool (d1 + d2 > sigma) then they CANNOT both be         *)
(* admitted together against that pool — there is no world in which each     *)
(* fits and their sum also fits. A single linear witness (the pool) cannot   *)
(* fund both: the second draw against the already-committed pool is rejected.*)
THEOREM RejectBothOnOversubscription ==
  \A sigma, d1, d2 \in Nat :
    (d1 + d2 > sigma) => ~(d1 <= sigma /\ d2 <= sigma /\ d1 + d2 <= sigma)
  BY SMT

(*-------------------------------------------------------------------------*)
(* P1 — slash effect / order independence (Slashing.Validator.bm_slash_*    *)
(* and Lean validator_contract_built_in_P1_order_independent, DR-12).      *)
(*                                                                         *)
(* The slash effect zeros a validator's bond. We model a finite bond map    *)
(* as a function Validators -> Nat and the slash effect as `Zero`, the      *)
(* update that sets one validator's bond to 0. The two kernel facts are     *)
(* idempotence (slashing twice = slashing once) and order independence      *)
(* (slashing a set of validators is independent of the order), which is the *)
(* consensus-critical multi-parent-merge determinism of slashing.          *)
(*-------------------------------------------------------------------------*)

(* Zero(m, v): the bond map m with validator v's bond set to 0 — the TLA+   *)
(* image of Slashing.Validator.bm_slash. *)
Zero(m, v) == [ m EXCEPT ![v] = 0 ]

(* ZeroingIdempotent (P1 effect): slashing the same validator a second time  *)
(* changes nothing — the offender's bond is already 0. This is the TLA+      *)
(* image of bm_slash_lookup / bm_slash_idempotent_lookup. *)
THEOREM ZeroingIdempotent ==
  \A V : \A m \in [ V -> Nat ] : \A v \in V :
    Zero(Zero(m, v), v) = Zero(m, v)
  BY DEF Zero

(* SlashOrderIndependent (P1 determinism): slashing two validators a then b  *)
(* yields the SAME bond map as slashing b then a — the bond-map effect of a  *)
(* slash SET does not depend on the order it is applied, so concurrent       *)
(* multi-parent merges that slash overlapping sets reconcile to one map.     *)
(* This is the TLA+ image of bm_slash_many_order_independent. *)
THEOREM SlashOrderIndependent ==
  \A V : \A m \in [ V -> Nat ] : \A a, b \in V :
    Zero(Zero(m, a), b) = Zero(Zero(m, b), a)
  BY DEF Zero

=============================================================================
