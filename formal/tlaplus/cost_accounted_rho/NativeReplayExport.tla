-------------------- MODULE NativeReplayExport --------------------
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Workers, MaxRestores, AllowIncomplete, EarlyRelease, SkipCommit, AllowReuse
VARIABLES stage, granted, tuples, done, phase, capturedDone, capturedTuples,
          disk, exports, restores
vars == <<stage, granted, tuples, done, phase, capturedDone, capturedTuples,
          disk, exports, restores>>

Active == {w \in Workers : stage[w] \in {"Prepared", "Applied"}}
MayMutate == phase = "Idle" \/ (EarlyRelease /\ phase = "CaptureState")

Init ==
  /\ stage = [w \in Workers |-> "Idle"]
  /\ granted \in SUBSET Workers
  /\ tuples = {} /\ done = {}
  /\ phase = "Idle"
  /\ capturedDone = {} /\ capturedTuples = {}
  /\ disk = {} /\ exports = <<>> /\ restores = 0

Prepare(w) ==
  /\ MayMutate /\ stage[w] = "Idle"
  /\ stage' = [stage EXCEPT ![w] = "Prepared"]
  /\ UNCHANGED <<granted, tuples, done, phase, capturedDone, capturedTuples, disk, exports, restores>>

Apply(w) ==
  /\ MayMutate /\ stage[w] = "Prepared"
  /\ stage' = [stage EXCEPT ![w] = "Applied"]
  /\ tuples' = IF w \in granted THEN tuples \cup {w} ELSE tuples
  /\ UNCHANGED <<granted, done, phase, capturedDone, capturedTuples, disk, exports, restores>>

Publish(w) ==
  /\ MayMutate /\ stage[w] = "Applied"
  /\ stage' = [stage EXCEPT ![w] = "Done"]
  /\ done' = done \cup {w}
  /\ UNCHANGED <<granted, tuples, phase, capturedDone, capturedTuples, disk, exports, restores>>

Cancel(w) ==
  /\ MayMutate /\ stage[w] = "Prepared"
  /\ stage' = [stage EXCEPT ![w] = "Idle"]
  /\ UNCHANGED <<granted, tuples, done, phase, capturedDone, capturedTuples, disk, exports, restores>>

Restore ==
  /\ MayMutate /\ Active = {} /\ restores < MaxRestores
  /\ stage' = [w \in Workers |-> "Idle"]
  /\ tuples' = {} /\ done' = {} /\ restores' = restores + 1
  /\ UNCHANGED <<granted, phase, capturedDone, capturedTuples, disk, exports>>

BeginExport ==
  /\ phase = "Idle" /\ Active = {}
  /\ done = Workers \/ AllowIncomplete
  /\ capturedDone' = done
  /\ phase' = "CaptureState"
  /\ UNCHANGED <<stage, granted, tuples, done, capturedTuples, disk, exports, restores>>

CaptureState ==
  /\ phase = "CaptureState"
  /\ capturedTuples' = tuples
  /\ phase' = IF SkipCommit THEN "Seal" ELSE "Persist"
  /\ UNCHANGED <<stage, granted, tuples, done, capturedDone, disk, exports, restores>>

Persist ==
  /\ phase = "Persist"
  /\ disk' = disk \cup {capturedTuples}
  /\ phase' = "Seal"
  /\ UNCHANGED <<stage, granted, tuples, done, capturedDone, capturedTuples, exports, restores>>

Seal ==
  /\ phase = "Seal"
  /\ exports' = Append(exports, [ledger |-> capturedDone, root |-> capturedTuples])
  /\ phase' = IF AllowReuse THEN "Idle" ELSE "Closed"
  /\ UNCHANGED <<stage, granted, tuples, done, capturedDone, capturedTuples, disk, restores>>

Abort ==
  /\ phase \in {"CaptureState", "Persist", "Seal"}
  /\ phase' = "Closed"
  /\ UNCHANGED <<stage, granted, tuples, done, capturedDone, capturedTuples, disk, exports, restores>>

Next == (\E w \in Workers : Prepare(w) \/ Apply(w) \/ Publish(w) \/ Cancel(w))
     \/ Restore \/ BeginExport \/ CaptureState \/ Persist \/ Seal \/ Abort
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ stage \in [Workers -> {"Idle", "Prepared", "Applied", "Done"}]
  /\ granted \subseteq Workers /\ tuples \subseteq Workers /\ done \subseteq Workers
  /\ phase \in {"Idle", "CaptureState", "Persist", "Seal", "Closed"}
  /\ capturedDone \subseteq Workers /\ capturedTuples \subseteq Workers
  /\ disk \subseteq SUBSET Workers
  /\ exports \in Seq([ledger : SUBSET Workers, root : SUBSET Workers])
  /\ restores \in 0..MaxRestores
TupleLedgerAgreement ==
  tuples = (done \cup {w \in Workers : stage[w] = "Applied"}) \cap granted
CompleteEvidence == phase # "Idle" => capturedDone = Workers
CapturedAgreement == phase \in {"Persist", "Seal"} => capturedTuples = capturedDone \cap granted
ExportAgreement == \A i \in 1..Len(exports) :
  exports[i].ledger = Workers /\ exports[i].root = exports[i].ledger \cap granted
DurableExport == \A i \in 1..Len(exports) : exports[i].root \in disk
ExactlyOnce == Len(exports) <= 1
ClosedAccess == phase = "Closed" => ~MayMutate
=============================================================================
