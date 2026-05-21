------------------------ MODULE AuthorizedSlashFlow ------------------------
EXTENDS Integers, FiniteSets, TLC

CONSTANTS
    Validators,
    Hashes,
    Epochs,
    InitialBonds

VARIABLES
    bonds,
    lifetimeEpoch,
    evidence,
    pendingSlashDeploys,
    slashedSet,
    epoch,
    rejectedSlashDeploys,
    badAuthObserved

vars == <<bonds, lifetimeEpoch, evidence, pendingSlashDeploys,
          slashedSet, epoch, rejectedSlashDeploys, badAuthObserved>>

Evidence == Hashes \X Validators \X Epochs
SlashDeploy == Validators \X Epochs \X Hashes

Authorized(v, e, h) ==
    /\ <<h, v, e>> \in evidence
    /\ e = epoch
    /\ lifetimeEpoch[v] = e
    /\ bonds[v] > 0

TypeOK ==
    /\ bonds \in [Validators -> Nat]
    /\ lifetimeEpoch \in [Validators -> Epochs]
    /\ evidence \in SUBSET Evidence
    /\ pendingSlashDeploys \in SUBSET SlashDeploy
    /\ slashedSet \in SUBSET Validators
    /\ epoch \in Epochs
    /\ rejectedSlashDeploys \in SUBSET SlashDeploy
    /\ badAuthObserved \in BOOLEAN

Init ==
    /\ bonds = InitialBonds
    /\ lifetimeEpoch = [v \in Validators |-> CHOOSE e \in Epochs : TRUE]
    /\ evidence = {}
    /\ pendingSlashDeploys = {}
    /\ slashedSet = {}
    /\ epoch = CHOOSE e \in Epochs : TRUE
    /\ rejectedSlashDeploys = {}
    /\ badAuthObserved = FALSE

RecordSlashableInvalid(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ evidence' = evidence \cup {<<h, v, e>>}
    /\ pendingSlashDeploys' =
        IF e = epoch /\ lifetimeEpoch[v] = e /\ bonds[v] > 0
        THEN pendingSlashDeploys \cup {<<v, e, h>>}
        ELSE pendingSlashDeploys
    /\ UNCHANGED <<bonds, lifetimeEpoch, slashedSet, epoch, rejectedSlashDeploys, badAuthObserved>>

AdvanceEpoch(e) ==
    /\ e \in Epochs
    /\ epoch' = e
    /\ pendingSlashDeploys' =
        {<<ev[2], ev[3], ev[1]>> :
            ev \in {x \in evidence : x[3] = e /\ lifetimeEpoch[x[2]] = e /\ bonds[x[2]] > 0}}
    /\ UNCHANGED <<bonds, lifetimeEpoch, evidence, slashedSet, rejectedSlashDeploys, badAuthObserved>>

RebondSameKey(v) ==
    /\ v \in Validators
    /\ bonds[v] = 0
    /\ v \notin slashedSet
    /\ bonds' = [bonds EXCEPT ![v] = InitialBonds[v]]
    /\ lifetimeEpoch' = [lifetimeEpoch EXCEPT ![v] = epoch]
    /\ UNCHANGED <<evidence, pendingSlashDeploys, slashedSet, epoch, rejectedSlashDeploys, badAuthObserved>>

ReceiveUnauthorizedSlash(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ ~ Authorized(v, e, h)
    /\ rejectedSlashDeploys' = rejectedSlashDeploys \cup {<<v, e, h>>}
    /\ UNCHANGED <<bonds, lifetimeEpoch, evidence, pendingSlashDeploys, slashedSet, epoch, badAuthObserved>>

ReceiveBadAuthSlash(v, e, h) ==
    /\ v \in Validators
    /\ e \in Epochs
    /\ h \in Hashes
    /\ badAuthObserved' = TRUE
    /\ UNCHANGED <<bonds, lifetimeEpoch, evidence, pendingSlashDeploys, slashedSet, epoch, rejectedSlashDeploys>>

ExecuteSlash(v, e, h) ==
    /\ <<v, e, h>> \in pendingSlashDeploys
    /\ Authorized(v, e, h)
    /\ bonds' = [bonds EXCEPT ![v] = 0]
    /\ slashedSet' = slashedSet \cup {v}
    /\ pendingSlashDeploys' =
        {d \in pendingSlashDeploys : d[1] # v \/ d[2] # e}
    /\ UNCHANGED <<lifetimeEpoch, evidence, epoch, rejectedSlashDeploys, badAuthObserved>>

Next ==
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : RecordSlashableInvalid(v, e, h)
    \/ \E e \in Epochs : AdvanceEpoch(e)
    \/ \E v \in Validators : RebondSameKey(v)
    \/ \E v \in Validators, e \in Epochs, h \in Hashes : ReceiveUnauthorizedSlash(v, e, h)
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

============================================================================
