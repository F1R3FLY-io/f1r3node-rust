---------------------- MODULE StateBoundCandidateFunding ----------------------
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS First, Second, Left, Right, AllowStalePublication
Transactions == {First, Second}
Sources == {Left, Right}
ASSUME /\ First # Second /\ Left # Right
       /\ AllowStalePublication \in BOOLEAN

VARIABLES phase, source, capturedRoot, capturedBalance, balance,
          root, replayVerified, published, chargeCount, deliveries,
          totalDebit, abortEffectFree
vars == <<phase, source, capturedRoot, capturedBalance, balance,
          root, replayVerified, published, chargeCount, deliveries,
          totalDebit, abortEffectFree>>

Init ==
  /\ phase = [t \in Transactions |-> "Idle"]
  /\ source \in [Transactions -> Sources]
  /\ capturedRoot = [t \in Transactions |-> 0]
  /\ capturedBalance = [t \in Transactions |-> 0]
  /\ balance = [s \in Sources |-> 1]
  /\ root = 0
  /\ replayVerified = {}
  /\ published = {}
  /\ chargeCount = [t \in Transactions |-> 0]
  /\ deliveries = [t \in Transactions |-> 0]
  /\ totalDebit = 0
  /\ abortEffectFree = TRUE

Prepare(t) ==
  /\ phase[t] = "Idle"
  /\ phase' = [phase EXCEPT ![t] = "Prepared"]
  /\ capturedRoot' = [capturedRoot EXCEPT ![t] = root]
  /\ capturedBalance' = [capturedBalance EXCEPT ![t] = balance[source[t]]]
  /\ UNCHANGED <<source, balance, root, replayVerified, published,
                  chargeCount, deliveries, totalDebit, abortEffectFree>>

Validate(t) ==
  /\ phase[t] = "Prepared"
  /\ capturedBalance[t] >= 1
  /\ phase' = [phase EXCEPT ![t] = "Validated"]
  /\ replayVerified' = replayVerified \cup {t}
  /\ UNCHANGED <<source, capturedRoot, capturedBalance, balance, root,
                  published, chargeCount, deliveries, totalDebit, abortEffectFree>>

RejectUnderfunded(t) ==
  /\ phase[t] = "Prepared"
  /\ capturedBalance[t] = 0
  /\ phase' = [phase EXCEPT ![t] = "Rejected"]
  /\ UNCHANGED <<source, capturedRoot, capturedBalance, balance, root,
                  replayVerified, published, chargeCount, deliveries,
                  totalDebit, abortEffectFree>>

RefreshStale(t) ==
  /\ phase[t] \in {"Prepared", "Validated"}
  /\ capturedRoot[t] # root
  /\ phase' = [phase EXCEPT ![t] = "Prepared"]
  /\ capturedRoot' = [capturedRoot EXCEPT ![t] = root]
  /\ capturedBalance' = [capturedBalance EXCEPT ![t] = balance[source[t]]]
  /\ replayVerified' = replayVerified \ {t}
  /\ UNCHANGED <<source, balance, root, published, chargeCount, deliveries,
                  totalDebit, abortEffectFree>>

Publish(t) ==
  /\ phase[t] = "Validated"
  /\ t \in replayVerified
  /\ t \notin published
  /\ (AllowStalePublication \/
        (capturedRoot[t] = root /\ capturedBalance[t] = balance[source[t]]))
  /\ phase' = [phase EXCEPT ![t] = "Done"]
  /\ balance' = [balance EXCEPT ![source[t]] = capturedBalance[t] - 1]
  /\ root' = root + 1
  /\ published' = published \cup {t}
  /\ chargeCount' = [chargeCount EXCEPT ![t] = @ + 1]
  /\ totalDebit' = totalDebit + 1
  /\ deliveries' = [deliveries EXCEPT ![t] = 1]
  /\ UNCHANGED <<source, capturedRoot, capturedBalance, replayVerified,
                  abortEffectFree>>

Abort(t) ==
  /\ phase[t] \in {"Prepared", "Validated"}
  /\ phase' = [phase EXCEPT ![t] = "Aborted"]
  /\ replayVerified' = replayVerified \ {t}
  /\ abortEffectFree' = abortEffectFree /\ balance' = balance
       /\ root' = root /\ published' = published
       /\ chargeCount' = chargeCount /\ totalDebit' = totalDebit
  /\ UNCHANGED <<source, capturedRoot, capturedBalance, balance, root,
                  published, chargeCount, deliveries, totalDebit>>

DuplicateDelivery(t) ==
  /\ phase[t] = "Done"
  /\ deliveries[t] < 2
  /\ deliveries' = [deliveries EXCEPT ![t] = @ + 1]
  /\ UNCHANGED <<phase, source, capturedRoot, capturedBalance, balance,
                  root, replayVerified, published, chargeCount,
                  totalDebit, abortEffectFree>>

Next == \E t \in Transactions : Prepare(t) \/ Validate(t) \/
  RejectUnderfunded(t) \/ RefreshStale(t) \/ Publish(t) \/
  Abort(t) \/ DuplicateDelivery(t)
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in [Transactions ->
       {"Idle", "Prepared", "Validated", "Done", "Rejected", "Aborted"}]
  /\ balance \in [Sources -> Nat]
  /\ replayVerified \subseteq Transactions
  /\ published \subseteq Transactions
  /\ totalDebit \in Nat

NoDuplicateCharge == \A t \in Transactions : chargeCount[t] <= 1
BalanceConservation == balance[Left] + balance[Right] + totalDebit = 2
PublishedWasReplayVerified == published \subseteq replayVerified
AbortIsEffectFree == abortEffectFree
StaleCandidateCanRefresh == \A t \in Transactions :
  (phase[t] \in {"Prepared", "Validated"} /\ capturedRoot[t] # root)
  => ENABLED RefreshStale(t)
IndependentSourcesCanFinish ==
  (source[First] # source[Second] /\ phase[First] = "Done" /\
   phase[Second] = "Validated" /\ capturedRoot[Second] = root)
  => ENABLED Publish(Second)
IndependentStaleCanRebaseWithoutLosingFunding ==
  (source[First] # source[Second] /\ phase[First] = "Done" /\
   phase[Second] \in {"Prepared", "Validated"} /\
   capturedRoot[Second] # root)
  => (balance[source[Second]] = 1 /\ ENABLED RefreshStale(Second))

=============================================================================
