---------------------- MODULE BenchmarkCancellation ----------------------
EXTENDS Naturals, TLC

CONSTANTS WatchGuardian, FaultKinds, GraceTicks, ObservationTicks
VARIABLES phase, faultKind, guardianAlive, clientAlive, breachRecorded,
          termSent, summaryPublished, elapsed
vars == <<phase, faultKind, guardianAlive, clientAlive, breachRecorded,
          termSent, summaryPublished, elapsed>>

Init ==
    /\ phase = "active"
    /\ faultKind \in FaultKinds
    /\ guardianAlive = TRUE
    /\ clientAlive = TRUE
    /\ breachRecorded = FALSE
    /\ termSent = FALSE
    /\ summaryPublished = FALSE
    /\ elapsed = 0

Fault ==
    /\ phase = "active"
    /\ guardianAlive' = (faultKind # "death")
    /\ breachRecorded' = (faultKind = "breach")
    /\ phase' = "watch"
    /\ UNCHANGED <<faultKind, clientAlive, termSent, summaryPublished, elapsed>>

Watch ==
    /\ phase = "watch"
    /\ phase' = IF WatchGuardian THEN "term" ELSE "waiting"
    /\ breachRecorded' = IF WatchGuardian THEN breachRecorded \/ ~guardianAlive ELSE breachRecorded
    /\ UNCHANGED <<faultKind, guardianAlive, clientAlive, termSent, summaryPublished, elapsed>>

Term ==
    /\ phase = "term"
    /\ termSent' = TRUE
    /\ phase' = "grace"
    /\ UNCHANGED <<faultKind, guardianAlive, clientAlive, breachRecorded, summaryPublished, elapsed>>

WaitGrace ==
    /\ phase = "grace"
    /\ elapsed < GraceTicks
    /\ elapsed' = elapsed + 1
    /\ UNCHANGED <<phase, faultKind, guardianAlive, clientAlive, breachRecorded, termSent, summaryPublished>>

Kill ==
    /\ phase = "grace"
    /\ elapsed = GraceTicks
    /\ clientAlive' = FALSE
    /\ phase' = "publish"
    /\ UNCHANGED <<faultKind, guardianAlive, breachRecorded, termSent, summaryPublished, elapsed>>

Publish ==
    /\ phase = "publish"
    /\ summaryPublished' = TRUE
    /\ phase' = "observed"
    /\ UNCHANGED <<faultKind, guardianAlive, clientAlive, breachRecorded, termSent, elapsed>>

WaitUnwatched ==
    /\ phase = "waiting"
    /\ elapsed < ObservationTicks
    /\ elapsed' = elapsed + 1
    /\ UNCHANGED <<phase, faultKind, guardianAlive, clientAlive, breachRecorded, termSent, summaryPublished>>

ObserveUnwatched ==
    /\ phase = "waiting"
    /\ elapsed = ObservationTicks
    /\ phase' = "observed"
    /\ UNCHANGED <<faultKind, guardianAlive, clientAlive, breachRecorded, termSent, summaryPublished, elapsed>>

Next == Fault \/ Watch \/ Term \/ WaitGrace \/ Kill \/ Publish \/ WaitUnwatched \/ ObserveUnwatched
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ phase \in {"active", "watch", "term", "grace", "publish", "waiting", "observed"}
    /\ faultKind \in FaultKinds
    /\ guardianAlive \in BOOLEAN
    /\ clientAlive \in BOOLEAN
    /\ breachRecorded \in BOOLEAN
    /\ termSent \in BOOLEAN
    /\ summaryPublished \in BOOLEAN
    /\ elapsed \in 0..ObservationTicks

BenchmarkCancellationObserved ==
    phase = "observed" => (~clientAlive /\ breachRecorded /\ summaryPublished)

TermBeforeStop == ~clientAlive => termSent
Completes == <>(phase = "observed")
=============================================================================
