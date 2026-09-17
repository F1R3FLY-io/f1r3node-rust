------------------------ MODULE DeployOccurrence ------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS MaxDeploys, MaxSources, Validators, SourceAware

Deploys == 1..MaxDeploys
Sources == 1..MaxSources
Occurrences == [deploy : Deploys, source : Sources]

VARIABLE observed

vars == <<observed>>

Init == observed = [v \in Validators |-> {}]

Observe(v, o) ==
    /\ o \notin observed[v]
    /\ observed' = [observed EXCEPT ![v] = @ \union {o}]

Share(v, w) ==
    /\ v /= w
    /\ observed[w] /= observed[v] \union observed[w]
    /\ observed' = [observed EXCEPT ![w] = @ \union observed[v]]

Next ==
    (\E v \in Validators : \E o \in Occurrences : Observe(v, o))
    \/ (\E v \in Validators : \E w \in Validators : Share(v, w))

Spec == Init /\ [][Next]_vars

ObservedDeploys(v) == {d \in Deploys : \E o \in observed[v] : o.deploy = d}

SourcesFor(v, d) ==
    {source \in Sources : \E o \in observed[v] :
        /\ o.deploy = d
        /\ o.source = source}

MinSource(v, d) ==
    CHOOSE source \in SourcesFor(v, d) :
        \A other \in SourcesFor(v, d) : source <= other

Canonical(v) ==
    {o \in observed[v] : o.source = MinSource(v, o.deploy)}

DuplicateDeploys(v) ==
    {d \in Deploys : Cardinality(SourcesFor(v, d)) > 1}

Rejected(v) ==
    IF SourceAware
    THEN observed[v] \ Canonical(v)
    ELSE {o \in observed[v] : o.deploy \in DuplicateDeploys(v)}

Active(v) == observed[v] \ Rejected(v)

TypeOK ==
    observed \in [Validators -> SUBSET Occurrences]

Inv_ExactTombstones ==
    \A v \in Validators : Rejected(v) \subseteq observed[v]

Inv_UniqueActiveOccurrence ==
    \A v \in Validators : \A d \in Deploys :
        Cardinality({o \in Active(v) : o.deploy = d}) <= 1

Inv_OneWinnerPreserved ==
    \A v \in Validators : \A d \in ObservedDeploys(v) :
        Cardinality({o \in Active(v) : o.deploy = d}) = 1

Inv_ObservationOrderConverges ==
    \A v \in Validators : \A w \in Validators :
        observed[v] = observed[w] =>
            /\ Canonical(v) = Canonical(w)
            /\ Rejected(v) = Rejected(w)
            /\ Active(v) = Active(w)
=============================================================================
