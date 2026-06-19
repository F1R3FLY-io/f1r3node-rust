----------------------- MODULE JustificationProjection -----------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC

CONSTANTS
    Validators,
    MaxJustifications

VARIABLES
    justifications,
    accepted

vars == <<justifications, accepted>>

BoundedJustificationLists ==
    UNION {[1..n -> Validators] : n \in 0..MaxJustifications}

HasDuplicate(seq) ==
    \E i, j \in 1..Len(seq) :
        /\ i # j
        /\ seq[i] = seq[j]

UniqueValidators(seq) ==
    ~ HasDuplicate(seq)

ProjectedValidators(seq) ==
    {seq[i] : i \in 1..Len(seq)}

Accepts(seq) ==
    UniqueValidators(seq)

TypeOK ==
    /\ MaxJustifications \in Nat
    /\ justifications \in BoundedJustificationLists
    /\ accepted \in BOOLEAN

Init ==
    /\ justifications \in BoundedJustificationLists
    /\ accepted = Accepts(justifications)

LoadJustifications(seq) ==
    /\ seq \in BoundedJustificationLists
    /\ justifications' = seq
    /\ accepted' = Accepts(seq)

Next ==
    \E seq \in BoundedJustificationLists : LoadJustifications(seq)

Spec == Init /\ [][Next]_vars

Inv_DuplicateJustificationsRejected ==
    HasDuplicate(justifications) => accepted = FALSE

Inv_AcceptedImpliesUniqueJustifications ==
    accepted => UniqueValidators(justifications)

Inv_AcceptedProjectionCardinality ==
    accepted => Cardinality(ProjectedValidators(justifications)) = Len(justifications)

=============================================================================
