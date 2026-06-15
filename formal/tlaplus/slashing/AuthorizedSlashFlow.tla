------------------------ MODULE AuthorizedSlashFlow ------------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANTS
    Validators,
    Hashes,
    Epochs,
    InitialBonds

VARIABLES
    bonds,
    ambientBonds,
    parentBonds,
    lifetimeEpoch,
    evidence,
    pendingSlashDeploys,
    slashedSet,
    epoch,
    rejectedSlashDeploys,
    mergeRejectedSlashDeploys,
    recoveredSlashDeploys,
    badAuthObserved

vars == <<bonds, ambientBonds, parentBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
          slashedSet, epoch, rejectedSlashDeploys,
          mergeRejectedSlashDeploys, recoveredSlashDeploys,
          badAuthObserved>>

Evidence == Hashes \X Validators \X Epochs
SlashDeploy == Validators \X Epochs \X Hashes
BondValues == {0} \cup {InitialBonds[v] : v \in Validators}

AuthEvidence(v, e, h) ==
    /\ <<h, v, e>> \in evidence
    /\ e = epoch
    /\ lifetimeEpoch[v] = e

Authorized(v, e, h) ==
    /\ AuthEvidence(v, e, h)
    /\ parentBonds[v] > 0

TypeOK ==
    /\ bonds \in [Validators -> Nat]
    /\ ambientBonds \in [Validators -> Nat]
    /\ parentBonds \in [Validators -> Nat]
    /\ lifetimeEpoch \in [Validators -> Epochs]
    /\ evidence \in SUBSET Evidence
    /\ pendingSlashDeploys \in SUBSET SlashDeploy
    /\ slashedSet \in SUBSET Validators
    /\ epoch \in Epochs
    /\ rejectedSlashDeploys \in SUBSET SlashDeploy
    /\ mergeRejectedSlashDeploys \in SUBSET SlashDeploy
    /\ recoveredSlashDeploys \in SUBSET SlashDeploy
    /\ badAuthObserved \in BOOLEAN

Init ==
    /\ bonds = InitialBonds
    /\ ambientBonds = InitialBonds
    /\ parentBonds = InitialBonds
    /\ lifetimeEpoch = [v \in Validators |-> CHOOSE e \in Epochs : TRUE]
    /\ evidence = {}
    /\ pendingSlashDeploys = {}
    /\ slashedSet = {}
    /\ epoch = CHOOSE e \in Epochs : TRUE
    /\ rejectedSlashDeploys = {}
    /\ mergeRejectedSlashDeploys = {}
    /\ recoveredSlashDeploys = {}
    /\ badAuthObserved = FALSE

HashUnused(h) ==
    \A ev \in evidence : ev[1] # h

PendingCoversHash(h) ==
    \E d \in pendingSlashDeploys : d[3] = h

AuthorizedDeploysForView(view) ==
    {<<ev[2], ev[3], ev[1]>> :
        ev \in {x \in evidence :
            x[3] = epoch /\ lifetimeEpoch[x[2]] = x[3] /\ view[x[2]] > 0}}

RecordSlashableInvalid(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ HashUnused(h)
    /\ evidence' = evidence \cup {<<h, v, e>>}
    /\ pendingSlashDeploys' =
        IF e = epoch /\ lifetimeEpoch[v] = e /\ parentBonds[v] > 0 /\ ~ PendingCoversHash(h)
        THEN pendingSlashDeploys \cup {<<v, e, h>>}
        ELSE pendingSlashDeploys
    /\ UNCHANGED <<bonds, ambientBonds, parentBonds, lifetimeEpoch, slashedSet, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    recoveredSlashDeploys, badAuthObserved>>

AdvanceEpoch(e) ==
    /\ e \in Epochs
    /\ epoch' = e
    /\ pendingSlashDeploys' =
        {<<ev[2], ev[3], ev[1]>> :
            ev \in {x \in evidence : x[3] = e /\ lifetimeEpoch[x[2]] = e /\ parentBonds[x[2]] > 0}}
    /\ UNCHANGED <<bonds, ambientBonds, parentBonds, lifetimeEpoch, evidence, slashedSet,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    recoveredSlashDeploys, badAuthObserved>>

RebondSameKey(v) ==
    /\ v \in Validators
    /\ bonds[v] = 0
    /\ v \notin slashedSet
    /\ \A d \in pendingSlashDeploys : d[1] # v
    /\ bonds' = [bonds EXCEPT ![v] = InitialBonds[v]]
    /\ ambientBonds' = [ambientBonds EXCEPT ![v] = InitialBonds[v]]
    /\ parentBonds' = [parentBonds EXCEPT ![v] = InitialBonds[v]]
    /\ lifetimeEpoch' = [lifetimeEpoch EXCEPT ![v] = epoch]
    /\ UNCHANGED <<evidence, pendingSlashDeploys, slashedSet, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    recoveredSlashDeploys, badAuthObserved>>

SelectAmbientSnapshot(view) ==
    /\ view \in [Validators -> BondValues]
    /\ ambientBonds' = view
    /\ UNCHANGED <<bonds, parentBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
                    slashedSet, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, recoveredSlashDeploys, badAuthObserved>>

SelectParentPreState(view) ==
    /\ view \in [Validators -> BondValues]
    /\ parentBonds' = view
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(view)
    /\ UNCHANGED <<bonds, ambientBonds, lifetimeEpoch, evidence,
                    slashedSet, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, recoveredSlashDeploys, badAuthObserved>>

ReceiveUnauthorizedSlash(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ ~ Authorized(v, e, h)
    /\ rejectedSlashDeploys' = rejectedSlashDeploys \cup {<<v, e, h>>}
    /\ UNCHANGED <<bonds, ambientBonds, parentBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
                    slashedSet, epoch, mergeRejectedSlashDeploys,
                    recoveredSlashDeploys, badAuthObserved>>

ObserveMergeRejectedSlash(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ <<h, v, e>> \in evidence
    /\ mergeRejectedSlashDeploys' = mergeRejectedSlashDeploys \cup {<<v, e, h>>}
    /\ UNCHANGED <<bonds, ambientBonds, parentBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
                    slashedSet, epoch, rejectedSlashDeploys,
                    recoveredSlashDeploys, badAuthObserved>>

RecoverMergeRejectedSlash(v, e, h) ==
    /\ <<v, e, h>> \in mergeRejectedSlashDeploys
    /\ Authorized(v, e, h)
    /\ recoveredSlashDeploys' = recoveredSlashDeploys \cup {<<v, e, h>>}
    /\ pendingSlashDeploys' =
        IF PendingCoversHash(h)
        THEN pendingSlashDeploys
        ELSE pendingSlashDeploys \cup {<<v, e, h>>}
    /\ UNCHANGED <<bonds, ambientBonds, parentBonds, lifetimeEpoch, evidence, slashedSet, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys,
                    badAuthObserved>>

ReceiveBadAuthSlash(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ badAuthObserved' = TRUE
    /\ UNCHANGED <<bonds, ambientBonds, parentBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
                    slashedSet, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, recoveredSlashDeploys>>

ExecuteSlash(v, e, h) ==
    /\ <<v, e, h>> \in pendingSlashDeploys
    /\ Authorized(v, e, h)
    /\ IF bonds[v] > 0
       THEN
         /\ bonds' = [bonds EXCEPT ![v] = 0]
         /\ ambientBonds' = [ambientBonds EXCEPT ![v] = 0]
         /\ slashedSet' = slashedSet \cup {v}
         /\ pendingSlashDeploys' =
             {d \in pendingSlashDeploys : d[1] # v \/ d[2] # e}
       ELSE
         /\ bonds' = bonds
         /\ ambientBonds' = ambientBonds
         /\ slashedSet' = slashedSet
         /\ pendingSlashDeploys' = pendingSlashDeploys \ {<<v, e, h>>}
    /\ UNCHANGED <<parentBonds, lifetimeEpoch, evidence, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, recoveredSlashDeploys,
                    badAuthObserved>>

Next ==
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : RecordSlashableInvalid(v, e, h)
    \/ \E e \in Epochs : AdvanceEpoch(e)
    \/ \E v \in Validators : RebondSameKey(v)
    \/ \E view \in [Validators -> BondValues] : SelectAmbientSnapshot(view)
    \/ \E view \in [Validators -> BondValues] : SelectParentPreState(view)
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : ReceiveUnauthorizedSlash(v, e, h)
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : ObserveMergeRejectedSlash(v, e, h)
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : RecoverMergeRejectedSlash(v, e, h)
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : ReceiveBadAuthSlash(v, e, h)
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : ExecuteSlash(v, e, h)

Spec == Init /\ [][Next]_vars

Inv_StaleEvidenceCannotSlashRebondedKey ==
    \A ev \in evidence :
        LET h == ev[1]
            v == ev[2]
            e == ev[3]
        IN e # lifetimeEpoch[v] => <<v, e, h>> \notin pendingSlashDeploys

Inv_OnlyAuthorizedSlashCanBePending ==
    \A d \in pendingSlashDeploys :
        Authorized(d[1], d[2], d[3])

Inv_NoInvalidLatestLivenessGap ==
    \A ev \in evidence :
        LET h == ev[1]
            v == ev[2]
            e == ev[3]
        IN Authorized(v, e, h) =>
             (<<v, e, h>> \in pendingSlashDeploys \/ v \in slashedSet)

Inv_RejectedSlashWithoutEvidenceNoPending ==
    \A d \in rejectedSlashDeploys :
        <<d[3], d[1], d[2]>> \notin evidence => d \notin pendingSlashDeploys

Inv_InvalidAuthSlashNoPending ==
    badAuthObserved =>
        \A d \in pendingSlashDeploys :
            Authorized(d[1], d[2], d[3])

Inv_BondsZeroAfterSlash ==
    \A v \in slashedSet : bonds[v] = 0 \/ lifetimeEpoch[v] = epoch

Inv_EvidenceHashUnique ==
    \A ev1 \in evidence :
      \A ev2 \in evidence :
        ev1[1] = ev2[1] => ev1 = ev2

Inv_RecoveredSlashHasEvidence ==
    \A d \in recoveredSlashDeploys :
        <<d[3], d[1], d[2]>> \in evidence

Inv_RecoveredSlashCoveredByPendingOrExecuted ==
    \A d \in recoveredSlashDeploys :
        ~ Authorized(d[1], d[2], d[3]) \/ PendingCoversHash(d[3]) \/ d[1] \in slashedSet

Inv_AuthorizationUsesParentPreState ==
    \A ev \in evidence :
        LET h == ev[1]
            v == ev[2]
            e == ev[3]
        IN AuthEvidence(v, e, h) =>
             (Authorized(v, e, h) <=> parentBonds[v] > 0)

Inv_AmbientZeroDoesNotBlockParentPositiveAuth ==
    \A ev \in evidence :
        LET h == ev[1]
            v == ev[2]
            e == ev[3]
        IN /\ AuthEvidence(v, e, h)
           /\ ambientBonds[v] = 0
           /\ parentBonds[v] > 0
           => Authorized(v, e, h)

Inv_ParentZeroRejectsEvenAmbientPositive ==
    \A ev \in evidence :
        LET h == ev[1]
            v == ev[2]
            e == ev[3]
        IN /\ AuthEvidence(v, e, h)
           /\ parentBonds[v] = 0
           /\ ambientBonds[v] > 0
           => ~ Authorized(v, e, h)

Inv_PendingSlashHashUnique ==
    \A d1 \in pendingSlashDeploys :
      \A d2 \in pendingSlashDeploys :
        d1[3] = d2[3] => d1 = d2

============================================================================
