------------------------ MODULE PayloadSortPersistence ------------------------
EXTENDS Naturals, Sequences, TLC

CONSTANTS Authority, AlternateAuthority, StackHead, StackTail,
          EraseStoredMetadata, DropMatchedComponents, ReplayRebindsAuthority

ASSUME /\ Authority # AlternateAuthority
       /\ StackHead # StackTail
       /\ EraseStoredMetadata \in BOOLEAN
       /\ DropMatchedComponents \in BOOLEAN
       /\ ReplayRebindsAuthority \in BOOLEAN

VARIABLES phase, normalized, stored, captured, executed, replayed

vars == <<phase, normalized, stored, captured, executed, replayed>>

NoAuthority == "NoAuthority"

NoPayload ==
  [authority |-> NoAuthority, stack |-> <<>>, conditional |-> FALSE]

CompletePayload ==
  [authority |-> Authority,
   stack |-> <<StackHead, StackTail>>,
   conditional |-> TRUE]

Init ==
  /\ phase = "Store"
  /\ normalized = CompletePayload
  /\ stored = NoPayload
  /\ captured = NoPayload
  /\ executed = NoPayload
  /\ replayed = NoPayload

Store ==
  /\ phase = "Store"
  /\ stored' = IF EraseStoredMetadata THEN NoPayload ELSE normalized
  /\ phase' = "Capture"
  /\ UNCHANGED <<normalized, captured, executed, replayed>>

Capture ==
  /\ phase = "Capture"
  /\ captured' = IF DropMatchedComponents THEN NoPayload ELSE stored
  /\ phase' = "Execute"
  /\ UNCHANGED <<normalized, stored, executed, replayed>>

Execute ==
  /\ phase = "Execute"
  /\ executed' = captured
  /\ phase' = "Replay"
  /\ UNCHANGED <<normalized, stored, captured, replayed>>

Replay ==
  /\ phase = "Replay"
  /\ replayed' =
       IF ReplayRebindsAuthority
       THEN [executed EXCEPT !.authority = AlternateAuthority]
       ELSE executed
  /\ phase' = "Done"
  /\ UNCHANGED <<normalized, stored, captured, executed>>

Next == Store \/ Capture \/ Execute \/ Replay

Spec == /\ Init
        /\ [][Next]_vars
        /\ WF_vars(Store)
        /\ WF_vars(Capture)
        /\ WF_vars(Execute)
        /\ WF_vars(Replay)

PayloadType ==
  [authority : {NoAuthority, Authority, AlternateAuthority},
   stack : Seq({StackHead, StackTail}),
   conditional : BOOLEAN]

TypeOK ==
  /\ phase \in {"Store", "Capture", "Execute", "Replay", "Done"}
  /\ normalized \in PayloadType
  /\ stored \in PayloadType
  /\ captured \in PayloadType
  /\ executed \in PayloadType
  /\ replayed \in PayloadType

StoragePreservesCompletePayload ==
  phase \in {"Capture", "Execute", "Replay", "Done"} => stored = normalized

MatcherCaptureIsExact ==
  phase \in {"Execute", "Replay", "Done"} => captured = stored

ExecutionPreservesSignedAuthorityAndStackOrder ==
  phase \in {"Replay", "Done"} => executed = normalized

ReplayPreservesCompletePayload ==
  phase = "Done" => replayed = executed

EventuallyDone == <>(phase = "Done")

=============================================================================
