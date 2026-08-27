------------------------- MODULE DeployLifecycle -------------------------
(***************************************************************************)
(* Model of the F1R3FLY deploy lifecycle across block admission and       *)
(* finalization.                                                            *)
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
(*     process_finalized removes finalized deploys from deploy_storage but  *)
(*     deliberately retains the rejected buffer until admission can decide  *)
(*     the exact occurrence disposition.                                    *)
(*   casper/src/rust/util/rholang/interpreter_util.rs                       *)
(*     canonical_won_sigs filters canonically landed deploys at admission.   *)
(*                                                                         *)
(* `AdmissionFiltersFinalized` distinguishes the current admission filter   *)
(* from the regression in which every rejected-buffer member was treated as  *)
(* re-proposable even after canonical finalization. Retaining buffer records  *)
(* is required for keep-one recovery and is not itself re-proposal.          *)
(*                                                                         *)
(* The `QuarantineBothStores` constant gates the PROPOSER-SIDE quarantine of  *)
(* a refund-failing re-proposed deploy (a "toxic" deploy):                    *)
(*   QuarantineBothStores = TRUE  -> the FIX: quarantine purges the toxic     *)
(*     deploy from BOTH pending AND rejectedBuf; RecoverFromRecord cannot      *)
(*     re-derive it (no loser record), so it stays out of rejectedBuf         *)
(*     (Inv_NoToxicReproposable holds -- the RECOVERY path is clean).          *)
(*   QuarantineBothStores = FALSE -> the PRE-FIX: only pending is cleared, so  *)
(*     the toxic deploy LINGERS in rejectedBuf and recovery re-proposes it     *)
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
    AdmissionFiltersFinalized,
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
    toxic             \* observation set: deploys quarantined after a refund
                      \* failure. A refund-failed deploy produces NO block
                      \* (NoNewDeploys, block_creator.rs), hence NO loser record,
                      \* so recovery cannot re-derive it (RecoverFromRecord skips
                      \* toxic). The fix additionally removes it from rejectedBuf.
                      \* A client MAY re-submit the same sig (a fresh lifecycle);
                      \* the code does NOT permanently bar re-submission, so the
                      \* guarantee is scoped to the RECOVERY path (rejectedBuf).

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
        \* NB: a quarantined (toxic) deploy MAY be re-submitted -- the code does
        \* not permanently bar it (quarantine only removes it from the stores).
        \* A re-submitted toxic deploy is a fresh pending entry; if it refund-fails
        \* again it is re-quarantined. This is faithful: no `d \notin toxic` guard.
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
\* only be repopulated for it before finalization. Admission filters any
\* retained record after the canonical win.
RecoverFromRecord ==
    \E d \in accepted :
        /\ d \notin rejectedBuf
        /\ d \notin toxic
        /\ rejectedBuf' = rejectedBuf \union {d}
        /\ UNCHANGED <<pending, accepted, finalized, rejectedInBlock, blocksUsed, toxic>>

\* process_finalized removes finalized deploys from deploy_storage. Rejected
\* buffer entries remain available for occurrence-aware recovery; admission
\* decides whether an entry is actually re-proposable.
Finalize ==
    \E D \in (SUBSET accepted) \ {{}} :
      \E R \in SUBSET rejectedInBlock :
        /\ finalized' = finalized \union D
        /\ accepted' = accepted \ D
        /\ pending' = pending \ D
        /\ rejectedInBlock' = rejectedInBlock \ R
        /\ rejectedBuf' = rejectedBuf
        /\ UNCHANGED <<blocksUsed, toxic>>

\* Proposer-side quarantine of a refund-failing re-proposed deploy. A deploy that
\* was recovered into the rejected_deploy_buffer (RecoverFromRecord) and then
\* re-proposed can FAIL its refund on replay; such a "toxic" deploy must be
\* quarantined so it can never be re-proposed and re-fail. The fix removes it from
\* the pending pool AND -- gated by QuarantineBothStores -- from the buffer, and
\* records it in `toxic` (the poison set); the Submit / RecoverFromRecord guards
\* keep it permanently barred.
\*   QuarantineBothStores = TRUE  -> the FIX: purge BOTH stores; the toxic deploy
\*     is never in rejectedBuf again, so recovery cannot re-propose it
\*     (Inv_NoToxicReproposable holds -- scoped to the recovery path).
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

RejectedProposable ==
    IF AdmissionFiltersFinalized
    THEN rejectedBuf \ finalized
    ELSE rejectedBuf

\* A finalized deploy is absent from every proposable source. The rejected
\* buffer may retain its historical record; the admission projection excludes
\* it after canonical finalization.
Inv_NoFinalizedReproposable ==
    \A d \in finalized : d \notin pending /\ d \notin RejectedProposable

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
\* The proposer-side quarantine guarantee, scoped to the RECOVERY path (the actual
\* RCA): a refund-failing deploy, once quarantined, is not in rejectedBuf, so
\* record-driven recovery cannot re-propose it and re-fail (livelock). The pre-fix
\* (QuarantineBothStores=FALSE) leaves it in rejectedBuf -> violated. `pending` is
\* NOT asserted: the code permits a client to re-submit the same sig (a fresh
\* lifecycle), so barring it in `pending` would over-claim the fix.
Inv_NoToxicReproposable ==
    \A d \in toxic : d \notin rejectedBuf
==============================================================================
