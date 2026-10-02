---------------------- MODULE AtomicQuoteSharedCapacity -----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS
  \* @type: Int;
  First,
  \* @type: Int;
  Second,
  \* @type: Int;
  OperationA,
  \* @type: Int;
  OperationB,
  \* @type: Bool;
  AllowStalePublication
Candidates == {First, Second}
Operations == {OperationA, OperationB}
ASSUME /\ First # Second
       /\ OperationA # OperationB
       /\ AllowStalePublication \in BOOLEAN

VARIABLES
  \* @type: Int -> Str;
  phase,
  \* @type: Int -> Int;
  operation,
  \* @type: Int -> Int;
  demand,
  \* @type: Int -> Bool;
  authenticated,
  \* @type: Int -> Bool;
  replayEqual,
  \* @type: Int -> Int;
  capturedRoot,
  \* @type: Int -> Int;
  capturedProvider,
  \* @type: Int -> Int;
  capturedInput,
  \* @type: Int -> Int;
  capturedUse,
  \* @type: Int -> Int;
  stagedOutput,
  \* @type: Int -> Int;
  stagedInput,
  \* @type: Set(Int);
  replayVerified,
  \* @type: Int;
  providerBalance,
  \* @type: Int;
  inputBalance,
  \* @type: Int;
  quoteUsed,
  \* @type: Int;
  totalOutputSpent,
  \* @type: Int;
  totalInputSpent,
  \* @type: Int;
  root,
  \* @type: Set(Int);
  settled,
  \* @type: Set(Int);
  publishedBy,
  \* @type: Int -> Int;
  publishedRoot,
  \* @type: Int -> Int;
  chargeCount,
  \* @type: Int -> Int;
  deliveries,
  \* @type: Int -> Int;
  settlementOutput,
  \* @type: Int -> Int;
  deliveredOutput
vars == <<phase, operation, demand, authenticated, replayEqual,
          capturedRoot, capturedProvider, capturedInput, capturedUse,
          stagedOutput, stagedInput, replayVerified, providerBalance,
          inputBalance, quoteUsed, totalOutputSpent, totalInputSpent,
          root, settled, publishedBy, publishedRoot, chargeCount, deliveries,
          settlementOutput, deliveredOutput>>

QuoteInput(y) == IF y = 0 THEN 0 ELSE ((3 * y + 1) \div 2) + 1

Init ==
  /\ phase = [t \in Candidates |-> "Idle"]
  /\ operation \in [Candidates -> Operations]
  /\ demand \in [Candidates -> {1, 2}]
  /\ authenticated \in [Candidates -> BOOLEAN]
  /\ replayEqual \in [Candidates -> BOOLEAN]
  /\ capturedRoot = [t \in Candidates |-> 0]
  /\ capturedProvider = [t \in Candidates |-> 0]
  /\ capturedInput = [t \in Candidates |-> 0]
  /\ capturedUse = [t \in Candidates |-> 0]
  /\ stagedOutput = [t \in Candidates |-> 0]
  /\ stagedInput = [t \in Candidates |-> 0]
  /\ replayVerified = {}
  /\ providerBalance = 3
  /\ inputBalance = 8
  /\ quoteUsed = 0
  /\ totalOutputSpent = 0
  /\ totalInputSpent = 0
  /\ root = 0
  /\ settled = {}
  /\ publishedBy = {}
  /\ publishedRoot = [t \in Candidates |-> 0]
  /\ chargeCount = [o \in Operations |-> 0]
  /\ deliveries = [t \in Candidates |-> 0]
  /\ settlementOutput = [o \in Operations |-> 0]
  /\ deliveredOutput = [t \in Candidates |-> 0]

Prepare(t) ==
  /\ phase[t] = "Idle"
  /\ phase' = [phase EXCEPT ![t] = "Prepared"]
  /\ capturedRoot' = [capturedRoot EXCEPT ![t] = root]
  /\ capturedProvider' = [capturedProvider EXCEPT ![t] = providerBalance]
  /\ capturedInput' = [capturedInput EXCEPT ![t] = inputBalance]
  /\ capturedUse' = [capturedUse EXCEPT ![t] = quoteUsed]
  /\ UNCHANGED <<operation, demand, authenticated, replayEqual,
                  stagedOutput, stagedInput, replayVerified, providerBalance,
                  inputBalance, quoteUsed, totalOutputSpent, totalInputSpent,
                  root, settled, publishedBy, publishedRoot, chargeCount,
                  deliveries, settlementOutput, deliveredOutput>>

Stage(t) ==
  /\ phase[t] = "Prepared"
  /\ QuoteInput(demand[t]) <= 4
  /\ phase' = [phase EXCEPT ![t] = "Staged"]
  /\ stagedOutput' = [stagedOutput EXCEPT ![t] = demand[t]]
  /\ stagedInput' = [stagedInput EXCEPT ![t] = QuoteInput(demand[t])]
  /\ UNCHANGED <<operation, demand, authenticated, replayEqual,
                  capturedRoot, capturedProvider, capturedInput, capturedUse,
                  replayVerified, providerBalance, inputBalance, quoteUsed,
                  totalOutputSpent, totalInputSpent, root, settled,
                  publishedBy, publishedRoot, chargeCount, deliveries,
                  settlementOutput, deliveredOutput>>

Validate(t) ==
  /\ phase[t] = "Staged"
  /\ authenticated[t]
  /\ replayEqual[t]
  /\ capturedProvider[t] >= stagedOutput[t]
  /\ capturedInput[t] >= stagedInput[t]
  /\ capturedUse[t] + stagedOutput[t] <= 3
  /\ phase' = [phase EXCEPT ![t] = "Validated"]
  /\ replayVerified' = replayVerified \cup {t}
  /\ UNCHANGED <<operation, demand, authenticated, replayEqual,
                  capturedRoot, capturedProvider, capturedInput, capturedUse,
                  stagedOutput, stagedInput, providerBalance, inputBalance,
                  quoteUsed, totalOutputSpent, totalInputSpent, root, settled,
                  publishedBy, publishedRoot, chargeCount, deliveries,
                  settlementOutput, deliveredOutput>>

RefreshStale(t) ==
  /\ phase[t] \in {"Prepared", "Staged", "Validated"}
  /\ capturedRoot[t] # root
  /\ phase' = [phase EXCEPT ![t] = "Prepared"]
  /\ capturedRoot' = [capturedRoot EXCEPT ![t] = root]
  /\ capturedProvider' = [capturedProvider EXCEPT ![t] = providerBalance]
  /\ capturedInput' = [capturedInput EXCEPT ![t] = inputBalance]
  /\ capturedUse' = [capturedUse EXCEPT ![t] = quoteUsed]
  /\ stagedOutput' = [stagedOutput EXCEPT ![t] = 0]
  /\ stagedInput' = [stagedInput EXCEPT ![t] = 0]
  /\ replayVerified' = replayVerified \ {t}
  /\ UNCHANGED <<operation, demand, authenticated, replayEqual,
                  providerBalance, inputBalance, quoteUsed, totalOutputSpent,
                  totalInputSpent, root, settled, publishedBy, publishedRoot,
                  chargeCount, deliveries, settlementOutput,
                  deliveredOutput>>

Publish(t) ==
  /\ phase[t] = "Validated"
  /\ t \in replayVerified
  /\ operation[t] \notin settled
  /\ (AllowStalePublication \/
        (capturedRoot[t] = root /\
         capturedProvider[t] = providerBalance /\
         capturedInput[t] = inputBalance /\
         capturedUse[t] = quoteUsed))
  /\ phase' = [phase EXCEPT ![t] = "Done"]
  /\ providerBalance' = capturedProvider[t] - stagedOutput[t]
  /\ inputBalance' = capturedInput[t] - stagedInput[t]
  /\ quoteUsed' = capturedUse[t] + stagedOutput[t]
  /\ totalOutputSpent' = totalOutputSpent + stagedOutput[t]
  /\ totalInputSpent' = totalInputSpent + stagedInput[t]
  /\ root' = root + 1
  /\ settled' = settled \cup {operation[t]}
  /\ publishedBy' = publishedBy \cup {t}
  /\ publishedRoot' = [publishedRoot EXCEPT ![t] = root]
  /\ chargeCount' = [chargeCount EXCEPT ![operation[t]] = @ + 1]
  /\ deliveries' = [deliveries EXCEPT ![t] = 1]
  /\ settlementOutput' = [settlementOutput EXCEPT ![operation[t]] = stagedOutput[t]]
  /\ deliveredOutput' = [deliveredOutput EXCEPT ![t] = stagedOutput[t]]
  /\ stagedOutput' = [stagedOutput EXCEPT ![t] = 0]
  /\ stagedInput' = [stagedInput EXCEPT ![t] = 0]
  /\ UNCHANGED <<operation, demand, authenticated, replayEqual,
                  capturedRoot, capturedProvider, capturedInput, capturedUse,
                  replayVerified>>

Duplicate(t) ==
  /\ phase[t] \in {"Idle", "Prepared", "Staged", "Validated"}
  /\ operation[t] \in settled
  /\ phase' = [phase EXCEPT ![t] = "Done"]
  /\ stagedOutput' = [stagedOutput EXCEPT ![t] = 0]
  /\ stagedInput' = [stagedInput EXCEPT ![t] = 0]
  /\ replayVerified' = replayVerified \ {t}
  /\ deliveries' = [deliveries EXCEPT ![t] = 1]
  /\ deliveredOutput' = [deliveredOutput EXCEPT ![t] = settlementOutput[operation[t]]]
  /\ UNCHANGED <<operation, demand, authenticated, replayEqual,
                  capturedRoot, capturedProvider, capturedInput, capturedUse,
                  providerBalance, inputBalance, quoteUsed, totalOutputSpent,
                  totalInputSpent, root, settled, publishedBy, publishedRoot,
                  chargeCount, settlementOutput>>

Redeliver(t) ==
  /\ phase[t] = "Done"
  /\ deliveries[t] < 2
  /\ deliveries' = [deliveries EXCEPT ![t] = @ + 1]
  /\ UNCHANGED <<phase, operation, demand, authenticated, replayEqual,
                  capturedRoot, capturedProvider, capturedInput, capturedUse,
                  stagedOutput, stagedInput, replayVerified, providerBalance,
                  inputBalance, quoteUsed, totalOutputSpent, totalInputSpent,
                  root, settled, publishedBy, publishedRoot, chargeCount,
                  settlementOutput, deliveredOutput>>

Abort(t) ==
  /\ phase[t] \in {"Prepared", "Staged", "Validated"}
  /\ phase' = [phase EXCEPT ![t] = "Aborted"]
  /\ stagedOutput' = [stagedOutput EXCEPT ![t] = 0]
  /\ stagedInput' = [stagedInput EXCEPT ![t] = 0]
  /\ replayVerified' = replayVerified \ {t}
  /\ UNCHANGED <<operation, demand, authenticated, replayEqual,
                  capturedRoot, capturedProvider, capturedInput, capturedUse,
                  providerBalance, inputBalance, quoteUsed, totalOutputSpent,
                  totalInputSpent, root, settled, publishedBy, publishedRoot,
                  chargeCount, deliveries, settlementOutput,
                  deliveredOutput>>

Next == \E t \in Candidates : Prepare(t) \/ Stage(t) \/ Validate(t) \/
  RefreshStale(t) \/ Publish(t) \/ Duplicate(t) \/ Redeliver(t) \/ Abort(t)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Candidates ->
       {"Idle", "Prepared", "Staged", "Validated", "Done", "Aborted"}]
  /\ operation \in [Candidates -> Operations]
  /\ demand \in [Candidates -> {1, 2}]
  /\ providerBalance \in Nat
  /\ inputBalance \in Nat
  /\ quoteUsed \in Nat
  /\ settled \subseteq Operations
  /\ publishedBy \subseteq Candidates

PhysicalConservation ==
  /\ providerBalance + totalOutputSpent = 3
  /\ inputBalance + totalInputSpent = 8
  /\ quoteUsed = totalOutputSpent

QuoteCapacity == quoteUsed <= 3
NoDuplicateCharge == \A o \in Operations : chargeCount[o] <= 1
AtomicAbort == \A t \in Candidates :
  phase[t] = "Aborted" =>
    (stagedOutput[t] = 0 /\ stagedInput[t] = 0 /\ t \notin publishedBy)
PublishedIsAuthenticated == \A t \in publishedBy :
  authenticated[t] /\ replayEqual[t] /\ t \in replayVerified
FreshPublication == \A t \in publishedBy :
  publishedRoot[t] = capturedRoot[t]
OneCandidatePerOperation == \A o \in Operations :
  Cardinality({t \in publishedBy : operation[t] = o}) <= 1
DuplicateHasPriorSettlement == \A t \in Candidates :
  (phase[t] = "Done" /\ t \notin publishedBy) => operation[t] \in settled
DuplicateReturnsAcceptedOutput == \A t \in Candidates :
  phase[t] = "Done" => deliveredOutput[t] = settlementOutput[operation[t]]

=============================================================================
