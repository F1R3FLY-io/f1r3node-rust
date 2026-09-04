-------------------- MODULE DeployOccurrenceStorage --------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS MaxDeploys, MaxSources, Validators, AtomicCommit, StrictActivation

Deploys == 1..MaxDeploys
Sources == 1..MaxSources
Occurrences == [deploy : Deploys, source : Sources]
NoSource == 0

VARIABLES activated, legacy, archive, open, terminal, summary, frozen,
          active, horizon, lifecycleOpen, lifecycleTerminal, pending

vars == <<activated, legacy, archive, open, terminal, summary, frozen,
          active, horizon, lifecycleOpen, lifecycleTerminal, pending>>

SourcesFor(rows, deploy) ==
    {o.source : o \in {item \in rows : item.deploy = deploy}}

MaxSource(sources) ==
    IF sources = {}
    THEN NoSource
    ELSE CHOOSE source \in sources : \A other \in sources : source >= other

Canonical(rows, deploy) == MaxSource(SourcesFor(rows, deploy))

EmptySummary == [deploy \in Deploys |-> NoSource]

Init ==
    /\ activated = [v \in Validators |-> FALSE]
    /\ legacy = [v \in Validators |-> FALSE]
    /\ archive = [v \in Validators |-> {}]
    /\ open = [v \in Validators |-> {}]
    /\ terminal = [v \in Validators |-> {}]
    /\ summary = [v \in Validators |-> EmptySummary]
    /\ frozen = [v \in Validators |-> EmptySummary]
    /\ active = [v \in Validators |-> EmptySummary]
    /\ horizon = [v \in Validators |-> EmptySummary]
    /\ lifecycleOpen = [v \in Validators |-> {}]
    /\ lifecycleTerminal = [v \in Validators |-> {}]
    /\ pending = [v \in Validators |-> {}]

StageLegacy(v) ==
    /\ ~activated[v]
    /\ ~legacy[v]
    /\ legacy' = [legacy EXCEPT ![v] = TRUE]
    /\ UNCHANGED <<activated, archive, open, terminal, summary, frozen,
                    active, horizon, lifecycleOpen, lifecycleTerminal, pending>>

StagePartial(v, occurrence) ==
    /\ ~activated[v]
    /\ occurrence \notin archive[v]
    /\ archive' = [archive EXCEPT ![v] = @ \union {occurrence}]
    /\ UNCHANGED <<activated, legacy, open, terminal, summary, frozen,
                    active, horizon, lifecycleOpen, lifecycleTerminal, pending>>

Activate(v) ==
    /\ ~activated[v]
    /\ IF StrictActivation THEN ~legacy[v] /\ archive[v] = {} ELSE TRUE
    /\ activated' = [activated EXCEPT ![v] = TRUE]
    /\ UNCHANGED <<legacy, archive, open, terminal, summary, frozen,
                    active, horizon, lifecycleOpen, lifecycleTerminal, pending>>

BeginInsert(v, occurrence) ==
    /\ activated[v]
    /\ pending[v] = {}
    /\ pending' = [pending EXCEPT ![v] = {occurrence}]
    /\ UNCHANGED <<activated, legacy, archive, open, terminal, summary,
                    frozen, active, horizon, lifecycleOpen, lifecycleTerminal>>

CommitInsert(v) ==
    /\ pending[v] /= {}
    /\ LET occurrence == CHOOSE item \in pending[v] : TRUE
           deploy == occurrence.deploy
           nextArchive == archive[v] \union {occurrence}
           nextSummary == Canonical(nextArchive, deploy)
       IN /\ archive' = [archive EXCEPT ![v] = nextArchive]
          /\ IF AtomicCommit
                THEN /\ summary' = [summary EXCEPT ![v][deploy] = nextSummary]
                     /\ open' = [open EXCEPT ![v] = IF deploy \in terminal[v]
                                                        THEN @
                                                        ELSE @ \union {deploy}]
                     /\ active' = [active EXCEPT ![v][deploy] =
                            IF deploy \in terminal[v] /\ nextSummary <= horizon[v][deploy]
                            THEN NoSource
                            ELSE nextSummary]
                     /\ lifecycleOpen' = [lifecycleOpen EXCEPT ![v] =
                            IF deploy \in lifecycleTerminal[v]
                            THEN @
                            ELSE @ \union {deploy}]
                ELSE /\ UNCHANGED <<summary, open, active, lifecycleOpen>>
          /\ pending' = [pending EXCEPT ![v] = {}]
          /\ UNCHANGED <<activated, legacy, terminal, frozen, horizon,
                          lifecycleTerminal>>

Crash(v) ==
    /\ pending[v] /= {}
    /\ pending' = [pending EXCEPT ![v] = {}]
    /\ UNCHANGED <<activated, legacy, archive, open, terminal, summary,
                    frozen, active, horizon, lifecycleOpen, lifecycleTerminal>>

Compact(v, deploy) ==
    /\ activated[v]
    /\ deploy \in open[v]
    /\ pending[v] = {}
    /\ terminal' = [terminal EXCEPT ![v] = @ \union {deploy}]
    /\ open' = [open EXCEPT ![v] = @ \ {deploy}]
    /\ frozen' = [frozen EXCEPT ![v][deploy] = summary[v][deploy]]
    /\ active' = [active EXCEPT ![v][deploy] = NoSource]
    /\ horizon' = [horizon EXCEPT ![v][deploy] = summary[v][deploy]]
    /\ lifecycleTerminal' = [lifecycleTerminal EXCEPT ![v] = @ \union {deploy}]
    /\ lifecycleOpen' = [lifecycleOpen EXCEPT ![v] = @ \ {deploy}]
    /\ UNCHANGED <<activated, legacy, archive, summary, pending>>

Read(v) ==
    /\ activated[v]
    /\ UNCHANGED vars

Next ==
    (\E v \in Validators : StageLegacy(v))
    \/ (\E v \in Validators : \E occurrence \in Occurrences : StagePartial(v, occurrence))
    \/ (\E v \in Validators : Activate(v))
    \/ (\E v \in Validators : \E occurrence \in Occurrences : BeginInsert(v, occurrence))
    \/ (\E v \in Validators : CommitInsert(v))
    \/ (\E v \in Validators : Crash(v))
    \/ (\E v \in Validators : \E deploy \in Deploys : Compact(v, deploy))
    \/ (\E v \in Validators : Read(v))

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ activated \in [Validators -> BOOLEAN]
    /\ legacy \in [Validators -> BOOLEAN]
    /\ archive \in [Validators -> SUBSET Occurrences]
    /\ open \in [Validators -> SUBSET Deploys]
    /\ terminal \in [Validators -> SUBSET Deploys]
    /\ summary \in [Validators -> [Deploys -> 0..MaxSources]]
    /\ frozen \in [Validators -> [Deploys -> 0..MaxSources]]
    /\ active \in [Validators -> [Deploys -> 0..MaxSources]]
    /\ horizon \in [Validators -> [Deploys -> 0..MaxSources]]
    /\ lifecycleOpen \in [Validators -> SUBSET Deploys]
    /\ lifecycleTerminal \in [Validators -> SUBSET Deploys]
    /\ pending \in [Validators -> SUBSET Occurrences]
    /\ \A v \in Validators : Cardinality(pending[v]) <= 1

Inv_FreshActivation ==
    \A v \in Validators : activated[v] => ~legacy[v]

Inv_SummaryMatchesArchive ==
    \A v \in Validators : \A deploy \in Deploys :
        activated[v] => summary[v][deploy] = Canonical(archive[v], deploy)

Inv_ExactlyOneDisposition ==
    \A v \in Validators : \A deploy \in Deploys :
        activated[v] /\ (SourcesFor(archive[v], deploy) /= {}) =>
            ((deploy \in open[v]) \/ (deploy \in terminal[v]))
            /\ ~((deploy \in open[v]) /\ (deploy \in terminal[v]))

Inv_ActiveMatchesDisposition ==
    \A v \in Validators : \A deploy \in Deploys :
        activated[v] =>
            /\ (deploy \in open[v] => active[v][deploy] = summary[v][deploy])
            /\ (deploy \in terminal[v] =>
                  active[v][deploy] = IF summary[v][deploy] <= horizon[v][deploy]
                                         THEN NoSource
                                         ELSE summary[v][deploy])

Inv_LifecycleAtomic ==
    \A v \in Validators :
        activated[v] =>
            /\ lifecycleOpen[v] = open[v]
            /\ lifecycleTerminal[v] = terminal[v]

Inv_FrozenIsArchived ==
    \A v \in Validators : \A deploy \in terminal[v] :
        activated[v] => frozen[v][deploy] \in SourcesFor(archive[v], deploy)

Inv_ReplicaConvergence ==
    \A v \in Validators : \A w \in Validators :
        activated[v] /\ activated[w]
        /\ archive[v] = archive[w]
        /\ terminal[v] = terminal[w]
        /\ horizon[v] = horizon[w] =>
            /\ summary[v] = summary[w]
            /\ active[v] = active[w]
            /\ lifecycleOpen[v] = lifecycleOpen[w]
            /\ lifecycleTerminal[v] = lifecycleTerminal[w]
=============================================================================
