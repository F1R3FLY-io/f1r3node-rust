---------------------- MODULE FinalizationGenesisIdentity ----------------------
EXTENDS FiniteSets, Integers, TLC

CONSTANT
    \* @type: Int;
    MaxRounds,
    \* @type: Int;
    MaxAssertions,
    \* @type: Bool;
    UnsafeResetAdvancedHead,
    \* @type: Bool;
    UnsafeOverwriteGenesis,
    \* @type: Bool;
    UnsafeSplitBootstrap,
    \* @type: Bool;
    UnsafeBackfillUnrootedHead,
    \* @type: Bool;
    UnsafeMissingLedgerMapping

ASSUME /\ MaxRounds \in Nat \ {0}
       /\ MaxAssertions \in Nat \ {0}
       /\ UnsafeResetAdvancedHead \in BOOLEAN
       /\ UnsafeOverwriteGenesis \in BOOLEAN
       /\ UnsafeSplitBootstrap \in BOOLEAN
       /\ UnsafeBackfillUnrootedHead \in BOOLEAN
       /\ UnsafeMissingLedgerMapping \in BOOLEAN

Clients == {1, 2}
Workers == {1, 2}
Rounds == 1..MaxRounds
GenesisIds == {1, 2}
CanonicalGenesis == 1
ConflictingGenesis == 2
CursorKinds == {"Projection", "Effects", "Compaction"}

VARIABLES
    \* @type: Bool;
    ledgerStoreRegistered,
    \* @type: Bool;
    dagConstructed,
    \* @type: Bool;
    anchorPresent,
    \* @type: Int;
    anchorGenesis,
    \* @type: Bool;
    headPresent,
    \* @type: Int;
    headGenesis,
    \* @type: Int;
    durableHead,
    \* @type: Int;
    headHighWater,
    \* @type: Set(Int);
    records,
    \* @type: Set(Str);
    cursorsPresent,
    \* @type: Int -> Int;
    acceptedAssertions,
    \* @type: Int -> Int;
    rejectedAssertions,
    \* @type: Int -> Int;
    restarts

vars == <<ledgerStoreRegistered, dagConstructed,
          anchorPresent, anchorGenesis, headPresent, headGenesis,
          durableHead, headHighWater, records, cursorsPresent,
          acceptedAssertions, rejectedAssertions, restarts>>

\* @type: (Int) => Set(Int);
Prefix(revision) == {round \in Rounds : round <= revision}

Pristine ==
    /\ ~anchorPresent
    /\ anchorGenesis = 0
    /\ ~headPresent
    /\ headGenesis = 0
    /\ durableHead = 0
    /\ records = {}
    /\ cursorsPresent = {}

Initialized ==
    /\ anchorPresent
    /\ headPresent
    /\ cursorsPresent = CursorKinds

Init ==
    /\ ledgerStoreRegistered = TRUE
    /\ dagConstructed = FALSE
    /\ anchorPresent = FALSE
    /\ anchorGenesis = 0
    /\ headPresent = FALSE
    /\ headGenesis = 0
    /\ durableHead = 0
    /\ headHighWater = 0
    /\ records = {}
    /\ cursorsPresent = {}
    /\ acceptedAssertions = [client \in Clients |-> 0]
    /\ rejectedAssertions = [client \in Clients |-> 0]
    /\ restarts = [client \in Clients |-> 0]

ConstructDag ==
    /\ ledgerStoreRegistered
    /\ ~dagConstructed
    /\ dagConstructed' = TRUE
    /\ UNCHANGED <<ledgerStoreRegistered, anchorPresent, anchorGenesis,
                    headPresent, headGenesis, durableHead, headHighWater,
                    records, cursorsPresent, acceptedAssertions,
                    rejectedAssertions, restarts>>

EnsurePristine(client) ==
    /\ dagConstructed
    /\ Pristine
    /\ acceptedAssertions[client] < MaxAssertions
    /\ anchorPresent' = TRUE
    /\ anchorGenesis' = CanonicalGenesis
    /\ headPresent' = TRUE
    /\ headGenesis' = CanonicalGenesis
    /\ cursorsPresent' = CursorKinds
    /\ acceptedAssertions' =
         [acceptedAssertions EXCEPT ![client] = @ + 1]
    /\ UNCHANGED <<ledgerStoreRegistered, dagConstructed,
                    durableHead, headHighWater, records,
                    rejectedAssertions, restarts>>

EnsureExact(client) ==
    /\ Initialized
    /\ anchorGenesis = CanonicalGenesis
    /\ headGenesis = anchorGenesis
    /\ acceptedAssertions[client] < MaxAssertions
    /\ acceptedAssertions' =
         [acceptedAssertions EXCEPT ![client] = @ + 1]
    /\ UNCHANGED <<ledgerStoreRegistered, dagConstructed,
                    anchorPresent, anchorGenesis, headPresent, headGenesis,
                    durableHead, headHighWater, records, cursorsPresent,
                    rejectedAssertions, restarts>>

RejectConflict(client) ==
    /\ Initialized
    /\ rejectedAssertions[client] < MaxAssertions
    /\ rejectedAssertions' =
         [rejectedAssertions EXCEPT ![client] = @ + 1]
    /\ UNCHANGED <<ledgerStoreRegistered, dagConstructed,
                    anchorPresent, anchorGenesis, headPresent, headGenesis,
                    durableHead, headHighWater, records, cursorsPresent,
                    acceptedAssertions, restarts>>

AppendRound(worker) ==
    /\ worker \in Workers
    /\ Initialized
    /\ anchorGenesis = CanonicalGenesis
    /\ headGenesis = anchorGenesis
    /\ durableHead < MaxRounds
    /\ records' = records \cup {durableHead + 1}
    /\ durableHead' = durableHead + 1
    /\ headHighWater' = headHighWater + 1
    /\ UNCHANGED <<ledgerStoreRegistered, dagConstructed,
                    anchorPresent, anchorGenesis, headPresent, headGenesis,
                    cursorsPresent, acceptedAssertions, rejectedAssertions,
                    restarts>>

Restart(client) ==
    /\ dagConstructed
    /\ Initialized
    /\ restarts[client] < MaxAssertions
    /\ restarts' = [restarts EXCEPT ![client] = @ + 1]
    /\ UNCHANGED <<ledgerStoreRegistered, dagConstructed,
                    anchorPresent, anchorGenesis, headPresent, headGenesis,
                    durableHead, headHighWater, records, cursorsPresent,
                    acceptedAssertions, rejectedAssertions>>

UnsafeReset ==
    /\ UnsafeResetAdvancedHead
    /\ Initialized
    /\ durableHead > 0
    /\ durableHead' = 0
    /\ records' = {}
    /\ UNCHANGED <<ledgerStoreRegistered, dagConstructed,
                    anchorPresent, anchorGenesis, headPresent, headGenesis,
                    headHighWater, cursorsPresent, acceptedAssertions,
                    rejectedAssertions, restarts>>

UnsafeOverwrite ==
    /\ UnsafeOverwriteGenesis
    /\ Initialized
    /\ anchorGenesis = CanonicalGenesis
    /\ anchorGenesis' = ConflictingGenesis
    /\ headGenesis' = ConflictingGenesis
    /\ UNCHANGED <<ledgerStoreRegistered, dagConstructed,
                    anchorPresent, headPresent, durableHead, headHighWater,
                    records, cursorsPresent, acceptedAssertions,
                    rejectedAssertions, restarts>>

UnsafeSplit ==
    /\ UnsafeSplitBootstrap
    /\ Pristine
    /\ anchorPresent' = TRUE
    /\ anchorGenesis' = CanonicalGenesis
    /\ UNCHANGED <<ledgerStoreRegistered, dagConstructed,
                    headPresent, headGenesis, durableHead, headHighWater,
                    records, cursorsPresent, acceptedAssertions,
                    rejectedAssertions, restarts>>

UnsafeBackfill ==
    /\ UnsafeBackfillUnrootedHead
    /\ Pristine
    /\ headPresent' = TRUE
    /\ headGenesis' = CanonicalGenesis
    /\ durableHead' = 1
    /\ headHighWater' = 1
    /\ records' = {1}
    /\ cursorsPresent' = CursorKinds
    /\ UNCHANGED <<ledgerStoreRegistered, dagConstructed,
                    anchorPresent, anchorGenesis, acceptedAssertions,
                    rejectedAssertions, restarts>>

UnsafeConstructWithoutLedgerStore ==
    /\ UnsafeMissingLedgerMapping
    /\ ~dagConstructed
    /\ ledgerStoreRegistered' = FALSE
    /\ dagConstructed' = TRUE
    /\ UNCHANGED <<anchorPresent, anchorGenesis, headPresent, headGenesis,
                    durableHead, headHighWater, records, cursorsPresent,
                    acceptedAssertions, rejectedAssertions, restarts>>

Next ==
    \/ ConstructDag
    \/ \E client \in Clients : EnsurePristine(client)
    \/ \E client \in Clients : EnsureExact(client)
    \/ \E client \in Clients : RejectConflict(client)
    \/ \E worker \in Workers : AppendRound(worker)
    \/ \E client \in Clients : Restart(client)
    \/ UnsafeReset
    \/ UnsafeOverwrite
    \/ UnsafeSplit
    \/ UnsafeBackfill
    \/ UnsafeConstructWithoutLedgerStore

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ ledgerStoreRegistered \in BOOLEAN
    /\ dagConstructed \in BOOLEAN
    /\ anchorPresent \in BOOLEAN
    /\ anchorGenesis \in 0..2
    /\ headPresent \in BOOLEAN
    /\ headGenesis \in 0..2
    /\ durableHead \in 0..MaxRounds
    /\ headHighWater \in 0..MaxRounds
    /\ records \in SUBSET Rounds
    /\ cursorsPresent \in SUBSET CursorKinds
    /\ acceptedAssertions \in [Clients -> 0..MaxAssertions]
    /\ rejectedAssertions \in [Clients -> 0..MaxAssertions]
    /\ restarts \in [Clients -> 0..MaxAssertions]

Inv_AtomicBootstrap == Pristine \/ Initialized
Inv_CanonicalGenesis == ~anchorPresent \/ anchorGenesis = CanonicalGenesis
Inv_HeadRooted == ~headPresent \/ (anchorPresent /\ headGenesis = anchorGenesis)
Inv_RecordPrefix == records = Prefix(durableHead)
Inv_HeadMonotonic == durableHead = headHighWater
Inv_ConstructedHasLedgerStore == ~dagConstructed \/ ledgerStoreRegistered
Inv_FreshConstructionHasNoHead ==
    (dagConstructed /\ ~anchorPresent) =>
      (~headPresent /\ durableHead = 0 /\ records = {})

Safety ==
    /\ TypeOK
    /\ Inv_AtomicBootstrap
    /\ Inv_CanonicalGenesis
    /\ Inv_HeadRooted
    /\ Inv_RecordPrefix
    /\ Inv_HeadMonotonic
    /\ Inv_ConstructedHasLedgerStore
    /\ Inv_FreshConstructionHasNoHead

=============================================================================
