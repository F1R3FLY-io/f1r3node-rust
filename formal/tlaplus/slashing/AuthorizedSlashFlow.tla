------------------------ MODULE AuthorizedSlashFlow ------------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANTS
    Validators,
    Hashes,
    Epochs,
    InitialBonds,
    ProposerUsesCanonicalPreState,
    ReceiverUsesCanonicalPreState

VARIABLES
    bonds,
    ambientBonds,
    canonicalBonds,
    lifetimeEpoch,
    evidence,
    pendingSlashDeploys,
    slashedSet,
    epoch,
    rejectedSlashDeploys,
    mergeRejectedSlashDeploys,
    badAuthObserved

vars == <<bonds, ambientBonds, canonicalBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
          slashedSet, epoch, rejectedSlashDeploys,
          mergeRejectedSlashDeploys, badAuthObserved>>

Evidence == Hashes \X Validators \X Epochs
SlashDeploy == Validators \X Epochs \X Hashes
BondValues == {0} \cup {InitialBonds[v] : v \in Validators}

AuthEvidence(v, e, h) ==
    /\ <<h, v, e>> \in evidence
    /\ e = epoch
    /\ lifetimeEpoch[v] = e

AuthorizedForView(view, v, e, h) ==
    /\ AuthEvidence(v, e, h)
    /\ view[v] > 0

Authorized(v, e, h) == AuthorizedForView(canonicalBonds, v, e, h)

ProposerAuthorityBonds ==
    IF ProposerUsesCanonicalPreState THEN canonicalBonds ELSE ambientBonds

ReceiverAuthorityBonds ==
    IF ReceiverUsesCanonicalPreState THEN canonicalBonds ELSE ambientBonds

ProposerAuthorized(v, e, h) == AuthorizedForView(ProposerAuthorityBonds, v, e, h)

ReceiverAuthorized(v, e, h) == AuthorizedForView(ReceiverAuthorityBonds, v, e, h)

TypeOK ==
    /\ bonds \in [Validators -> Nat]
    /\ ambientBonds \in [Validators -> Nat]
    /\ canonicalBonds \in [Validators -> Nat]
    /\ lifetimeEpoch \in [Validators -> Epochs]
    /\ evidence \in SUBSET Evidence
    /\ pendingSlashDeploys \in SUBSET SlashDeploy
    /\ slashedSet \in SUBSET Validators
    /\ epoch \in Epochs
    /\ rejectedSlashDeploys \in SUBSET SlashDeploy
    /\ mergeRejectedSlashDeploys \in SUBSET SlashDeploy
    /\ badAuthObserved \in BOOLEAN
    /\ ProposerUsesCanonicalPreState \in BOOLEAN
    /\ ReceiverUsesCanonicalPreState \in BOOLEAN

Init ==
    /\ bonds = InitialBonds
    /\ ambientBonds = InitialBonds
    /\ canonicalBonds = InitialBonds
    /\ lifetimeEpoch = [v \in Validators |-> CHOOSE e \in Epochs : TRUE]
    /\ evidence = {}
    /\ pendingSlashDeploys = {}
    /\ slashedSet = {}
    /\ epoch = CHOOSE e \in Epochs : TRUE
    /\ rejectedSlashDeploys = {}
    /\ mergeRejectedSlashDeploys = {}
    /\ badAuthObserved = FALSE

HashUnused(h) ==
    \A ev \in evidence : ev[1] # h

PendingCoversHash(h) ==
    \E d \in pendingSlashDeploys : d[3] = h

PendingCoversTarget(v, e) ==
    \E d \in pendingSlashDeploys : d[1] = v /\ d[2] = e

EvidenceHashesFor(evs, v, e) ==
    {h \in Hashes : <<h, v, e>> \in evs}

CanonicalEvidenceHash(evs, v, e) ==
    CHOOSE h \in EvidenceHashesFor(evs, v, e) : TRUE

AuthorizedTargetsForView(view, evs, currentEpoch, lifetimes) ==
    {v \in Validators :
        /\ EvidenceHashesFor(evs, v, currentEpoch) # {}
        /\ lifetimes[v] = currentEpoch
        /\ view[v] > 0}

AuthorizedDeploysForView(view, evs, currentEpoch, lifetimes) ==
    {<<v, currentEpoch, CanonicalEvidenceHash(evs, v, currentEpoch)>> :
        v \in AuthorizedTargetsForView(view, evs, currentEpoch, lifetimes)}

RecordSlashableInvalid(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ HashUnused(h)
    /\ evidence' = evidence \cup {<<h, v, e>>}
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(
        ProposerAuthorityBonds, evidence \cup {<<h, v, e>>}, epoch, lifetimeEpoch)
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, lifetimeEpoch, slashedSet, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys, badAuthObserved>>

AdvanceEpoch(e) ==
    /\ e \in Epochs
    /\ epoch' = e
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(
        ProposerAuthorityBonds, evidence, e, lifetimeEpoch)
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, lifetimeEpoch, evidence, slashedSet,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys, badAuthObserved>>

RebondSameKey(v) ==
    /\ v \in Validators
    /\ bonds[v] = 0
    /\ v \notin slashedSet
    /\ \A d \in pendingSlashDeploys : d[1] # v
    /\ bonds' = [bonds EXCEPT ![v] = InitialBonds[v]]
    /\ ambientBonds' = [ambientBonds EXCEPT ![v] = InitialBonds[v]]
    /\ canonicalBonds' = [canonicalBonds EXCEPT ![v] = InitialBonds[v]]
    /\ lifetimeEpoch' = [lifetimeEpoch EXCEPT ![v] = epoch]
    /\ UNCHANGED <<evidence, pendingSlashDeploys, slashedSet, epoch,
                    rejectedSlashDeploys, mergeRejectedSlashDeploys, badAuthObserved>>

SelectAmbientSnapshot(view) ==
    /\ view \in [Validators -> BondValues]
    /\ ambientBonds' = view
    /\ UNCHANGED <<bonds, canonicalBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
                    slashedSet, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, badAuthObserved>>

SelectCanonicalPreState(view) ==
    /\ view \in [Validators -> BondValues]
    /\ canonicalBonds' = view
    /\ pendingSlashDeploys' = AuthorizedDeploysForView(
        IF ProposerUsesCanonicalPreState THEN view ELSE ambientBonds,
        evidence, epoch, lifetimeEpoch)
    /\ UNCHANGED <<bonds, ambientBonds, lifetimeEpoch, evidence,
                    slashedSet, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, badAuthObserved>>

ReceiveUnauthorizedSlash(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ ~ ReceiverAuthorized(v, e, h)
    /\ rejectedSlashDeploys' = rejectedSlashDeploys \cup {<<v, e, h>>}
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
                    slashedSet, epoch, mergeRejectedSlashDeploys, badAuthObserved>>

ObserveMergeRejectedSlash(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ <<h, v, e>> \in evidence
    /\ mergeRejectedSlashDeploys' = mergeRejectedSlashDeploys \cup {<<v, e, h>>}
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
                    slashedSet, epoch, rejectedSlashDeploys,
                    badAuthObserved>>

ReceiveBadAuthSlash(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ badAuthObserved' = TRUE
    /\ UNCHANGED <<bonds, ambientBonds, canonicalBonds, lifetimeEpoch, evidence, pendingSlashDeploys,
                    slashedSet, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys>>

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
    /\ UNCHANGED <<canonicalBonds, lifetimeEpoch, evidence, epoch, rejectedSlashDeploys,
                    mergeRejectedSlashDeploys, badAuthObserved>>

Next ==
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : RecordSlashableInvalid(v, e, h)
    \/ \E e \in Epochs : AdvanceEpoch(e)
    \/ \E v \in Validators : RebondSameKey(v)
    \/ \E view \in [Validators -> BondValues] : SelectAmbientSnapshot(view)
    \/ \E view \in [Validators -> BondValues] : SelectCanonicalPreState(view)
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : ReceiveUnauthorizedSlash(v, e, h)
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : ObserveMergeRejectedSlash(v, e, h)
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
             (PendingCoversTarget(v, e) \/ v \in slashedSet)

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

Inv_MergeRejectedSlashCoveredByCanonicalScan ==
    \A d \in mergeRejectedSlashDeploys :
        Authorized(d[1], d[2], d[3]) => PendingCoversTarget(d[1], d[2]) \/ d[1] \in slashedSet

Inv_MergeRejectedSlashCannotAuthorizeZeroBond ==
    \A d \in mergeRejectedSlashDeploys :
        canonicalBonds[d[1]] = 0 => ~ PendingCoversTarget(d[1], d[2])

Inv_AuthorizationUsesCanonicalPreState ==
    \A ev \in evidence :
        LET h == ev[1]
            v == ev[2]
            e == ev[3]
        IN AuthEvidence(v, e, h) =>
             (Authorized(v, e, h) <=> canonicalBonds[v] > 0)

Inv_AmbientZeroDoesNotBlockCanonicalPositiveAuth ==
    \A ev \in evidence :
        LET h == ev[1]
            v == ev[2]
            e == ev[3]
        IN /\ AuthEvidence(v, e, h)
           /\ ambientBonds[v] = 0
           /\ canonicalBonds[v] > 0
           => Authorized(v, e, h)

Inv_CanonicalZeroRejectsEvenAmbientPositive ==
    \A ev \in evidence :
        LET h == ev[1]
            v == ev[2]
            e == ev[3]
        IN /\ AuthEvidence(v, e, h)
           /\ canonicalBonds[v] = 0
           /\ ambientBonds[v] > 0
           => ~ Authorized(v, e, h)

Inv_ProposerAuthorizationMatchesCanonical ==
    \A v \in Validators, e \in Epochs, h \in Hashes :
        ProposerAuthorized(v, e, h) <=> Authorized(v, e, h)

Inv_ReceiverAuthorizationMatchesCanonical ==
    \A v \in Validators, e \in Epochs, h \in Hashes :
        ReceiverAuthorized(v, e, h) <=> Authorized(v, e, h)

Inv_ProposerReceiverAuthorizationParity ==
    \A v \in Validators, e \in Epochs, h \in Hashes :
        ProposerAuthorized(v, e, h) <=> ReceiverAuthorized(v, e, h)

Inv_PendingSlashHashUnique ==
    \A d1 \in pendingSlashDeploys :
      \A d2 \in pendingSlashDeploys :
        d1[3] = d2[3] => d1 = d2

Inv_PendingSlashTargetUnique ==
    \A d1 \in pendingSlashDeploys :
      \A d2 \in pendingSlashDeploys :
        d1[1] = d2[1] /\ d1[2] = d2[2] => d1 = d2

============================================================================
