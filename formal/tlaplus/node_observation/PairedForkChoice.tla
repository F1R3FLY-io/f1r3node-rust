------------------------- MODULE PairedForkChoice -------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS Budget, Captures, Blocks, Bug
Modes == {"bounded", "reference"}
Floor == "floor"
None == "none"
Pending == [state |-> "pending", head |-> None, selected |-> FALSE, reason |-> None]

VARIABLES phase, capture, inputs, results, comparison, work, charged
vars == <<phase, capture, inputs, results, comparison, work, charged>>

Init == /\ phase = "idle"
        /\ capture = None
        /\ inputs = [m \in Modes |-> None]
        /\ results = [m \in Modes |-> Pending]
        /\ comparison = [state |-> "pending", headMatches |-> FALSE, reason |-> None]
        /\ work = [m \in Modes |-> 0]
        /\ charged = 0

Capture(c) == /\ phase = "idle" /\ c \in Captures
              /\ phase' = "bounded" /\ capture' = c
              /\ UNCHANGED <<inputs, results, comparison, work, charged>>

ReadCapture(m) == IF Bug \in {"digest", "compare"} /\ m = "reference"
                  THEN CHOOSE c \in Captures: c # capture
                  ELSE capture

Available(m, head, steps) ==
    /\ inputs' = [inputs EXCEPT ![m] = ReadCapture(m)]
    /\ results' = [results EXCEPT ![m] =
          [state |-> "available", head |-> head, selected |-> TRUE, reason |-> None]]
    /\ work' = [work EXCEPT ![m] = steps]
    /\ charged' = charged + steps

Unavailable(m, reason, steps) ==
    /\ inputs' = [inputs EXCEPT ![m] = ReadCapture(m)]
    /\ results' = [results EXCEPT ![m] =
          CASE Bug = "floor" ->
                 [state |-> "unavailable", head |-> Floor, selected |-> FALSE, reason |-> reason]
            [] Bug = "absent" ->
                 [state |-> "available", head |-> CHOOSE b \in Blocks: TRUE,
                  selected |-> FALSE, reason |-> None]
            [] OTHER ->
                 [state |-> "unavailable", head |-> None, selected |-> FALSE, reason |-> reason]]
    /\ work' = [work EXCEPT ![m] = steps]
    /\ charged' = charged + steps

NextPhase(m) == IF m = "bounded" THEN "reference" ELSE "compare"

Evaluate(m, head, steps) ==
    /\ phase = m /\ head \in Blocks /\ steps \in 1..Budget
    /\ phase' = NextPhase(m)
    /\ IF charged + steps <= Budget \/ Bug = "budget"
       THEN Available(m, head, steps)
       ELSE Unavailable(m, "limit", Budget - charged)
    /\ UNCHANGED <<capture, comparison>>

Refuse(m, reason) ==
    /\ phase = m /\ reason \in {"history_incomplete", "missing_body_coverage"}
    /\ phase' = NextPhase(m)
    /\ Unavailable(m, reason, 0)
    /\ UNCHANGED <<capture, comparison>>

BothAvailable == \A m \in Modes: results[m].state = "available"
SameInput == inputs["bounded"] = inputs["reference"]

Compare == /\ phase = "compare" /\ phase' = "done"
           /\ comparison' =
                IF BothAvailable /\ (SameInput \/ Bug = "compare")
                THEN [state |-> "available",
                      headMatches |-> results["bounded"].head = results["reference"].head,
                      reason |-> None]
                ELSE [state |-> "unavailable", headMatches |-> FALSE,
                      reason |-> IF ~BothAvailable THEN "result_unavailable"
                                 ELSE "input_digest_mismatch"]
           /\ UNCHANGED <<capture, inputs, results, work, charged>>

Next == \/ (\E c \in Captures: Capture(c))
        \/ (\E m \in Modes, head \in Blocks, steps \in 1..Budget: Evaluate(m, head, steps))
        \/ (\E m \in Modes, reason \in {"history_incomplete", "missing_body_coverage"}:
                Refuse(m, reason))
        \/ Compare

Spec == Init /\ [][Next]_vars

TypeOK == /\ phase \in {"idle", "bounded", "reference", "compare", "done"}
          /\ capture \in Captures \cup {None}
          /\ \A m \in Modes: inputs[m] \in Captures \cup {None}
          /\ \A m \in Modes: results[m].state \in {"pending", "available", "unavailable"}
          /\ \A m \in Modes: results[m].head \in Blocks \cup {None, Floor}
          /\ comparison.state \in {"pending", "available", "unavailable"}
          /\ \A m \in Modes: work[m] \in 0..(2 * Budget)
          /\ charged \in 0..(2 * Budget)

OneCapture == \A m \in Modes:
                  results[m].state # "pending" => inputs[m] = capture
HeadNotFloor == \A m \in Modes: results[m].head # Floor
NoFabricatedHead == \A m \in Modes:
                        results[m].state = "available" => results[m].selected
CompareSameInput == comparison.state = "available" => BothAvailable /\ SameInput
SharedBudget == /\ work["bounded"] + work["reference"] <= Budget
                /\ charged = work["bounded"] + work["reference"]
=============================================================================
