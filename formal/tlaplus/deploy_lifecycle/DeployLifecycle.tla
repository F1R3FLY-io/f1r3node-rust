------------------------- MODULE DeployLifecycle -------------------------
(***************************************************************************)
(* Faithful model of the F1R3FLY deploy lifecycle across block admission   *)
(* and finalization, written to DISCOVER (not assume) the re-proposal      *)
(* regression that the casper_engine split once introduced.                *)
(*                                                                         *)
(* Models:                                                                  *)
(*   casper/src/rust/engine/multi_parent_casper/block_admission.rs         *)
(*     add_deploy            (:143-169)  a deploy is put into deploy_storage *)
(*       (the PENDING pool).                                                 *)
(*     admit_handle_valid_block (:95-141) accepted deploys are DELIBERATELY  *)
(*       RETAINED in deploy_storage (NOT purged on mere DAG acceptance), so  *)
(*       an accepted-but-orphaned deploy can be re-proposed via the          *)
(*       canonical-won record before it is lost (:103-117).                  *)
(*   casper/src/rust/engine/multi_parent_casper/finalization_runner.rs      *)
(*     process_finalized (:204-256)  on finalization of a block there are    *)
(*       TWO purges:                                                         *)
(*         (1) deploy_storage.remove(block.body.deploys)          (:226)     *)
(*         (2) rejected_deploy_buffer purge of every finalized deploy sig    *)
(*             AND every block.body.rejected_deploys sig          (:250-255) *)
(*                                                                         *)
(* THE REGRESSION (documented at finalization_runner.rs:234-241): the       *)
(* casper_engine split DROPPED purge (2) when extracting the runner.        *)
(* Without it, record-driven recovery re-proposes an already-finalized       *)
(* deploy out of the buffer, double-applying it (a second write to a         *)
(* single-value cell -> IntegerAdd invariant violation under the             *)
(* convergence green-gate). It has been restored; this module keeps the      *)
(* regression reproducible and gates the fix.                               *)
(*                                                                         *)
(* The `PurgeRejectedBuf` constant is the knob that distinguishes the fix    *)
(* from the regression it replaced:                                          *)
(*   PurgeRejectedBuf = TRUE   -> the FIX: finalization purges the finalized  *)
(*     deploy sigs AND that block's rejected_deploys sigs from the buffer.   *)
(*   PurgeRejectedBuf = FALSE  -> the REGRESSION: purge (2) is gone, so a     *)
(*     finalized deploy that was recovered into the buffer LINGERS and stays  *)
(*     re-proposable (Inv_NoFinalizedReproposable is violated).              *)
(*                                                                         *)
(* The `QuarantineBothStores` constant gates the PROPOSER-SIDE quarantine of  *)
(* a refund-failing re-proposed deploy (a "toxic" deploy):                    *)
(*   QuarantineBothStores = TRUE  -> the FIX: quarantine purges the toxic     *)
(*     deploy from BOTH pending AND rejectedBuf and records it in `toxic`,    *)
(*     with the Submit / RecoverFromRecord guards keeping it barred           *)
(*     (Inv_NoToxicReproposable holds).                                       *)
(*   QuarantineBothStores = FALSE -> the PRE-FIX: only pending is cleared, so  *)
(*     the toxic deploy LINGERS in rejectedBuf and stays re-proposable        *)
(*     (Inv_NoToxicReproposable is violated).                                 *)
(*                                                                         *)
(* Deploys are abstracted by their signatures {1..MaxDeploys}. A "block" is  *)
(* a nonempty group of accepted deploys (its body.deploys) together with a    *)
(* disjoint set of rejected sigs (its body.rejected_deploys); MaxBlocks       *)
(* bounds the number of block-formation events so TLC terminates. The         *)
(* violation is qualitative and bound-independent.                          *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    MaxDeploys,          \* number of distinct deploy signatures (scaled, e.g. 3)
    MaxBlocks,           \* bound on block-formation events (scaled, e.g. 3)
    PurgeRejectedBuf,    \* TRUE = the fix; FALSE = the dropped-purge regression
    QuarantineBothStores \* TRUE = quarantine purges pending AND rejectedBuf (the
                         \* proposer-side fix); FALSE = the pre-fix that leaves the
                         \* toxic deploy re-proposable (Inv_NoToxicReproposable viol.)

\* Deploy signatures are abstracted as the naturals 1..MaxDeploys.
Deploys == 1..MaxDeploys

VARIABLES
    pending,          \* deploy_storage: submitted deploys awaiting finalization
    rejectedBuf,      \* rejected_deploy_buffer: the recovery re-proposal pool
    accepted,         \* deploys carried by an accepted (not-yet-finalized) block
    finalized,        \* signatures of deploys that landed in a finalized block
    rejectedInBlock,  \* body.rejected_deploys carried by accepted blocks
    blocksUsed,       \* auxiliary: number of blocks formed so far (<= MaxBlocks)
    toxic             \* quarantined refund-failing deploys (the poison set): once
                      \* toxic, a deploy is barred from every proposable store
                      \* (pending / rejectedBuf) AND from re-submission

vars == <<pending, rejectedBuf, accepted, finalized, rejectedInBlock, blocksUsed, toxic>>

Init ==
    /\ pending = {}
    /\ rejectedBuf = {}
    /\ accepted = {}
    /\ finalized = {}
    /\ rejectedInBlock = {}
    /\ blocksUsed = 0
    /\ toxic = {}

\* add_deploy: a client submits a fresh deploy into deploy_storage. The deploy
\* pool dedups against already-known signatures, so an already-finalized
\* (canonical) sig is NOT re-admitted here -- the only way a finalized sig can
\* reach a proposable buffer is the modelled recovery + missing-purge path,
\* never a plain resubmit.
Submit ==
    \E d \in Deploys :
        /\ d \notin pending
        /\ d \notin finalized
        /\ d \notin toxic
        /\ pending' = pending \union {d}
        /\ UNCHANGED <<rejectedBuf, accepted, finalized, rejectedInBlock, blocksUsed, toxic>>

\* admit_handle_valid_block: a block is formed from >= 1 pending deploys (its
\* body.deploys) plus a DISJOINT set of rejected sigs (its body.rejected_deploys,
\* drawn from the in-flight deploys it conflicts with). The included deploys
\* become `accepted` but are DELIBERATELY RETAINED in `pending`
\* (block_admission.rs:103-117) -- there is no purge on mere DAG acceptance.
AcceptIntoBlock ==
    /\ blocksUsed < MaxBlocks
    /\ \E incl \in (SUBSET pending) \ {{}} :
         \E rej \in SUBSET ((pending \union rejectedBuf) \ incl) :
            /\ accepted' = accepted \union incl
            /\ rejectedInBlock' = rejectedInBlock \union rej
            /\ blocksUsed' = blocksUsed + 1
            /\ UNCHANGED <<pending, rejectedBuf, finalized, toxic>>

\* Record-driven recovery (the canonical-won path): an accepted-but-orphaned
\* deploy is re-injected into rejected_deploy_buffer so it can be re-proposed
\* before it is lost. Enabled only while the deploy is still `accepted` (its
\* live orphan record); once finalized it leaves `accepted`, so the buffer can
\* only be repopulated for it BEFORE finalization -- exactly the window the
\* finalize purge must clean up.
RecoverFromRecord ==
    \E d \in accepted :
        /\ d \notin rejectedBuf
        /\ d \notin toxic
        /\ rejectedBuf' = rejectedBuf \union {d}
        /\ UNCHANGED <<pending, accepted, finalized, rejectedInBlock, blocksUsed, toxic>>

\* process_finalized: a chosen block's deploys become finalized. Purge (1)
\* always removes them from deploy_storage (pending). Purge (2) -- GATED BY
\* PurgeRejectedBuf -- removes the finalized sigs AND that block's
\* rejected_deploys sigs from the rejected_deploy_buffer. FALSE models the
\* casper_engine split that dropped purge (2).
Finalize ==
    \E D \in (SUBSET accepted) \ {{}} :
      \E R \in SUBSET rejectedInBlock :
        /\ finalized' = finalized \union D
        /\ accepted' = accepted \ D
        /\ pending' = pending \ D
        /\ rejectedInBlock' = rejectedInBlock \ R
        /\ rejectedBuf' = IF PurgeRejectedBuf
                            THEN rejectedBuf \ (D \union R)
                            ELSE rejectedBuf
        /\ UNCHANGED <<blocksUsed, toxic>>

\* Proposer-side quarantine of a refund-failing re-proposed deploy. A deploy that
\* was recovered into the rejected_deploy_buffer (RecoverFromRecord) and then
\* re-proposed can FAIL its refund on replay; such a "toxic" deploy must be
\* quarantined so it can never be re-proposed and re-fail. The fix removes it from
\* the pending pool AND -- gated by QuarantineBothStores -- from the buffer, and
\* records it in `toxic` (the poison set); the Submit / RecoverFromRecord guards
\* keep it permanently barred.
\*   QuarantineBothStores = TRUE  -> the FIX: purge BOTH stores; the toxic deploy
\*     is never in pending or rejectedBuf again (Inv_NoToxicReproposable holds).
\*   QuarantineBothStores = FALSE -> the PRE-FIX: only pending is cleared, so the
\*     toxic deploy LINGERS in rejectedBuf and stays re-proposable
\*     (Inv_NoToxicReproposable is violated).
QuarantineRefundFailure ==
    \E d \in rejectedBuf :
        /\ toxic' = toxic \union {d}
        /\ pending' = pending \ {d}
        /\ rejectedBuf' = IF QuarantineBothStores
                            THEN rejectedBuf \ {d}
                            ELSE rejectedBuf
        /\ UNCHANGED <<accepted, finalized, rejectedInBlock, blocksUsed>>

Next == Submit \/ AcceptIntoBlock \/ RecoverFromRecord \/ Finalize
        \/ QuarantineRefundFailure

Spec == Init /\ [][Next]_vars

------------------------------------------------------------------------------
TypeOK ==
    /\ pending \subseteq Deploys
    /\ rejectedBuf \subseteq Deploys
    /\ accepted \subseteq Deploys
    /\ finalized \subseteq Deploys
    /\ rejectedInBlock \subseteq Deploys
    /\ blocksUsed \in 0..MaxBlocks
    /\ toxic \subseteq Deploys

\* THE PROMISE the regression violated: a finalized deploy is never present in
\* any proposable buffer (deploy_storage or the rejected_deploy_buffer), so it
\* can never be re-proposed and double-applied. Holds for all reachable states
\* on the fix (PurgeRejectedBuf = TRUE); on the regression (FALSE) TLC discovers
\* a finalized deploy lingering in rejectedBuf via the recovery path.
Inv_NoFinalizedReproposable ==
    \A d \in finalized : d \notin pending /\ d \notin rejectedBuf

\* Retention / no-loss: an accepted, not-yet-finalized deploy is never lost --
\* it remains in deploy_storage OR in the recovery buffer. Guards the DUAL,
\* over-eager-purge bug (purging pending on mere acceptance); it holds
\* throughout because AcceptIntoBlock retains the deploy in `pending`. A
\* quarantined (toxic) deploy is EXEMPT: it is DELIBERATELY dropped from both
\* proposable stores (that is the point of the quarantine), so the no-loss
\* guarantee does not -- and must not -- cover it.
Inv_NoLossBeforeFinal ==
    \A d \in accepted :
        (d \notin finalized /\ d \notin toxic) => (d \in pending \/ d \in rejectedBuf)

\* THE promise the proposer-side quarantine keeps: a toxic (refund-failing,
\* quarantined) deploy is never present in any proposable buffer, so it can never
\* be re-proposed and re-fail its refund. Holds for all reachable states on the
\* fix (QuarantineBothStores = TRUE, which purges rejectedBuf too, plus the
\* Submit / RecoverFromRecord `d \notin toxic` guards). On the pre-fix
\* (QuarantineBothStores = FALSE) TLC discovers a toxic deploy lingering in
\* rejectedBuf via the recovery + partial-quarantine path.
Inv_NoToxicReproposable ==
    \A d \in toxic : d \notin rejectedBuf /\ d \notin pending
==============================================================================
