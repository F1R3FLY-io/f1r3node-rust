---------------------------- MODULE RecordProducer ----------------------------
CONSTANT RequireProducerSuccess
VARIABLES record, generated, producer, phase

vars == <<record, generated, producer, phase>>

Init ==
    /\ record = "previous"
    /\ generated = "empty"
    /\ producer = "pending"
    /\ phase = "generate"

Generate ==
    /\ phase = "generate"
    /\ \E outcome \in {"success", "empty-failure", "partial-failure"}:
        /\ producer' = IF outcome = "success" THEN "success" ELSE "failure"
        /\ generated' = CASE outcome = "success" -> "full"
                             [] outcome = "empty-failure" -> "empty"
                             [] OTHER -> "partial"
    /\ phase' = "publish"
    /\ UNCHANGED record

Publish ==
    /\ phase = "publish"
    /\ record' = IF RequireProducerSuccess /\ producer = "failure"
                  THEN record ELSE generated
    /\ phase' = "finished"
    /\ UNCHANGED <<generated, producer>>

Next == Generate \/ Publish
Spec == Init /\ [][Next]_vars /\ WF_vars(Next)

TypeOK ==
    /\ record \in {"previous", "empty", "partial", "full"}
    /\ generated \in {"empty", "partial", "full"}
    /\ producer \in {"pending", "success", "failure"}
    /\ phase \in {"generate", "publish", "finished"}

FailedProducerPreservesRecord ==
    (phase = "finished" /\ producer = "failure") => record = "previous"
SuccessfulProducerPublishes ==
    (phase = "finished" /\ producer = "success") => record = "full"
Completes == <>(phase = "finished")
=============================================================================
