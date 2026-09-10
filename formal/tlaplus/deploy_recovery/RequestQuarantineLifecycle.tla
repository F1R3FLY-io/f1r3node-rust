---------------------- MODULE RequestQuarantineLifecycle ---------------------
EXTENDS Naturals, FiniteSets

CONSTANTS
    \* @type: Set(Str);
    Hashes,
    \* @type: Int;
    MaxTracked,
    \* @type: Int;
    MaxAttempts,
    \* @type: Int;
    QuarantineTicks,
    \* @type: Int;
    MaxWaiters,
    \* @type: Bool;
    CleanupEvidenceOnExhaustion,
    \* @type: Bool;
    CleanupEvidenceOnReceipt,
    \* @type: Bool;
    PruneParentInsteadOfWaiter

Phases == {"idle", "active", "quarantined", "received", "admitted", "obsolete"}

VARIABLES
    \* @type: Str -> Str;
    phase,
    \* @type: Str -> Bool;
    unresolved,
    \* @type: Str -> Bool;
    dependencyEvidence,
    \* @type: Str -> Int;
    attempts,
    \* @type: Str -> Int;
    deadline,
    \* @type: Str -> Int;
    waiters,
    \* @type: Str -> Int;
    readyWaiters,
    \* @type: Int;
    now

vars ==
    <<phase, unresolved, dependencyEvidence, attempts, deadline,
      waiters, readyWaiters, now>>

Tracked == {hash \in Hashes : phase[hash] \in {"active", "quarantined", "received"}}

Init ==
    /\ phase = [hash \in Hashes |-> "idle"]
    /\ unresolved = [hash \in Hashes |-> FALSE]
    /\ dependencyEvidence = [hash \in Hashes |-> FALSE]
    /\ attempts = [hash \in Hashes |-> 0]
    /\ deadline = [hash \in Hashes |-> 0]
    /\ waiters = [hash \in Hashes |-> 0]
    /\ readyWaiters = [hash \in Hashes |-> 0]
    /\ now = 0

DiscoverOne(hash) ==
    /\ phase[hash] \in {"idle", "obsolete"}
    /\ Cardinality(Tracked) < MaxTracked
    /\ phase' = [phase EXCEPT ![hash] = "active"]
    /\ unresolved' = [unresolved EXCEPT ![hash] = TRUE]
    /\ dependencyEvidence' = [dependencyEvidence EXCEPT ![hash] = TRUE]
    /\ attempts' = [attempts EXCEPT ![hash] = 0]
    /\ deadline' = [deadline EXCEPT ![hash] = 0]
    /\ waiters' = [waiters EXCEPT ![hash] = MaxWaiters]
    /\ readyWaiters' = [readyWaiters EXCEPT ![hash] = 0]
    /\ UNCHANGED now

RetryOne(hash) ==
    /\ phase[hash] = "active"
    /\ attempts[hash] < MaxAttempts
    /\ attempts' = [attempts EXCEPT ![hash] = @ + 1]
    /\ UNCHANGED
        <<phase, unresolved, dependencyEvidence, deadline,
          waiters, readyWaiters, now>>

ExhaustOne(hash) ==
    /\ phase[hash] = "active"
    /\ attempts[hash] = MaxAttempts
    /\ phase' =
        [phase EXCEPT ![hash] =
            IF CleanupEvidenceOnExhaustion THEN "idle" ELSE "quarantined"]
    /\ dependencyEvidence' =
        [dependencyEvidence EXCEPT ![hash] =
            IF CleanupEvidenceOnExhaustion THEN FALSE ELSE @]
    /\ attempts' = [attempts EXCEPT ![hash] = 0]
    /\ deadline' = [deadline EXCEPT ![hash] = now + QuarantineTicks]
    /\ UNCHANGED <<unresolved, waiters, readyWaiters, now>>

ReceiveOne(hash) ==
    /\ phase[hash] \in {"active", "quarantined"}
    /\ phase' = [phase EXCEPT ![hash] = "received"]
    /\ dependencyEvidence' =
        [dependencyEvidence EXCEPT ![hash] =
            IF CleanupEvidenceOnReceipt THEN FALSE ELSE @]
    /\ attempts' = [attempts EXCEPT ![hash] = 0]
    /\ deadline' = [deadline EXCEPT ![hash] = 0]
    /\ UNCHANGED <<unresolved, waiters, readyWaiters, now>>

DeferOne(hash) ==
    /\ phase[hash] = "received"
    /\ phase' = [phase EXCEPT ![hash] = "active"]
    /\ attempts' = [attempts EXCEPT ![hash] = 0]
    /\ UNCHANGED
        <<unresolved, dependencyEvidence, deadline, waiters, readyWaiters, now>>

AdmitOne(hash) ==
    /\ phase[hash] = "received"
    /\ phase' = [phase EXCEPT ![hash] = "admitted"]
    /\ unresolved' = [unresolved EXCEPT ![hash] = FALSE]
    /\ dependencyEvidence' = [dependencyEvidence EXCEPT ![hash] = FALSE]
    /\ attempts' = [attempts EXCEPT ![hash] = 0]
    /\ deadline' = [deadline EXCEPT ![hash] = 0]
    /\ waiters' = [waiters EXCEPT ![hash] = 0]
    /\ readyWaiters' = [readyWaiters EXCEPT ![hash] = @ + waiters[hash]]
    /\ UNCHANGED now

CertifyObsoleteOne(hash) ==
    /\ phase[hash] \in {"active", "quarantined", "received"}
    /\ phase' = [phase EXCEPT ![hash] = "obsolete"]
    /\ unresolved' = [unresolved EXCEPT ![hash] = FALSE]
    /\ dependencyEvidence' = [dependencyEvidence EXCEPT ![hash] = FALSE]
    /\ attempts' = [attempts EXCEPT ![hash] = 0]
    /\ deadline' = [deadline EXCEPT ![hash] = 0]
    /\ waiters' = [waiters EXCEPT ![hash] = 0]
    /\ readyWaiters' = [readyWaiters EXCEPT ![hash] = 0]
    /\ UNCHANGED now

ExpireOne(hash) ==
    /\ phase[hash] = "quarantined"
    /\ now >= deadline[hash]
    /\ phase' = [phase EXCEPT ![hash] = "active"]
    /\ deadline' = [deadline EXCEPT ![hash] = 0]
    /\ UNCHANGED
        <<unresolved, dependencyEvidence, attempts, waiters, readyWaiters, now>>

PruneOne(hash) ==
    /\ phase[hash] \in {"active", "quarantined", "received"}
    /\ waiters[hash] > 1
    /\ IF PruneParentInsteadOfWaiter
       THEN
         /\ waiters' = [waiters EXCEPT ![hash] = 0]
         /\ readyWaiters' =
              [readyWaiters EXCEPT ![hash] = @ + waiters[hash]]
       ELSE
         /\ waiters' = [waiters EXCEPT ![hash] = @ - 1]
         /\ UNCHANGED readyWaiters
    /\ UNCHANGED
        <<phase, unresolved, dependencyEvidence, attempts, deadline, now>>

Tick ==
    /\ now < QuarantineTicks + MaxAttempts + 2
    /\ now' = now + 1
    /\ UNCHANGED
        <<phase, unresolved, dependencyEvidence, attempts, deadline,
          waiters, readyWaiters>>

Discover == \E hash \in Hashes : DiscoverOne(hash)
Retry == \E hash \in Hashes : RetryOne(hash)
Exhaust == \E hash \in Hashes : ExhaustOne(hash)
Receive == \E hash \in Hashes : ReceiveOne(hash)
Defer == \E hash \in Hashes : DeferOne(hash)
Admit == \E hash \in Hashes : AdmitOne(hash)
CertifyObsolete == \E hash \in Hashes : CertifyObsoleteOne(hash)
Expire == \E hash \in Hashes : ExpireOne(hash)
Prune == \E hash \in Hashes : PruneOne(hash)

Next ==
    Discover \/ Retry \/ Exhaust \/ Receive \/ Defer \/ Admit
    \/ CertifyObsolete \/ Expire \/ Prune \/ Tick

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ phase \in [Hashes -> Phases]
    /\ unresolved \in [Hashes -> BOOLEAN]
    /\ dependencyEvidence \in [Hashes -> BOOLEAN]
    /\ attempts \in [Hashes -> 0..MaxAttempts]
    /\ deadline \in [Hashes -> Nat]
    /\ waiters \in [Hashes -> 0..MaxWaiters]
    /\ readyWaiters \in [Hashes -> 0..MaxWaiters]
    /\ now \in Nat

Inv_UnresolvedHasEvidence ==
    \A hash \in Hashes : unresolved[hash] => dependencyEvidence[hash]

Inv_EvidenceHasTrackedOwner ==
    \A hash \in Hashes :
        dependencyEvidence[hash]
        => phase[hash] \in {"active", "quarantined", "received"}

Inv_QuarantinePreservesEvidence ==
    \A hash \in Hashes : phase[hash] = "quarantined" => dependencyEvidence[hash]

Inv_QuarantineSuppressesAttempts ==
    \A hash \in Hashes : phase[hash] = "quarantined" => attempts[hash] = 0

Inv_ReceiptPreservesEvidence ==
    \A hash \in Hashes : phase[hash] = "received" => dependencyEvidence[hash]

Inv_TrackingIsBounded == Cardinality(Tracked) <= MaxTracked

Inv_UnresolvedWaitersHaveEvidence ==
    \A hash \in Hashes :
        unresolved[hash] /\ waiters[hash] > 0 => dependencyEvidence[hash]

Inv_MissingParentNeverReadiesWaiter ==
    \A hash \in Hashes : unresolved[hash] => readyWaiters[hash] = 0

=============================================================================
