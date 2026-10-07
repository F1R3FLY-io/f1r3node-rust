------------------ MODULE MeteredSubstitutionWorklist ------------------
EXTENDS Integers, Sequences, FiniteSets

\* Bounded abstraction of CostSignature Compound -> Quote -> Par frames.
\* Rust's differential tests check the concrete per-frame reserve formulas;
\* this machine checks their DFS order, worklist expansion, and denial prefix.
CONSTANTS InitialCredit, MaxDepth
ASSUME InitialCredit \in Nat /\ MaxDepth \in Nat

Nodes == 1..5
Children == [n \in Nodes |->
    CASE n = 1 -> <<2, 3>>
      [] n = 2 -> <<4>>
      [] n = 4 -> <<5>>
      [] OTHER -> <<>>]
Node(n) == [kind |-> "node", node |-> n]
Enter(n) == [kind |-> "enter", node |-> n]
Exit(n) == [kind |-> "exit", node |-> n]

RECURSIVE SourceTrace(_), SourceChildren(_,_), ChildTasks(_,_), WorkTrace(_)
SourceChildren(children, index) ==
    IF index > Len(children) THEN <<>>
    ELSE SourceTrace(children[index]) \o SourceChildren(children, index + 1)
SourceTrace(node) ==
    <<Enter(node)>> \o SourceChildren(Children[node], 1) \o <<Exit(node)>>
ChildTasks(children, index) ==
    IF index > Len(children) THEN <<>>
    ELSE <<Node(children[index])>> \o ChildTasks(children, index + 1)
WorkTrace(pending) ==
    IF Len(pending) = 0 THEN <<>>
    ELSE (IF Head(pending).kind = "node"
          THEN SourceTrace(Head(pending).node)
          ELSE <<Head(pending)>>) \o WorkTrace(Tail(pending))

Expected == SourceTrace(1)
VARIABLES work, trace, credit, depth, phase
vars == <<work, trace, credit, depth, phase>>

Init ==
    /\ work = <<Node(1)>>
    /\ trace = <<>>
    /\ credit = InitialCredit
    /\ depth = 0
    /\ phase = "ready"

\* The production PDA pushes children in reverse onto a tail-top Vec. This
\* head-top abstraction has the equivalent source-order residual work.
Expand ==
    /\ phase = "ready" /\ Len(work) > 0 /\ Head(work).kind = "node"
    /\ work' = <<Enter(Head(work).node)>>
               \o ChildTasks(Children[Head(work).node], 1)
               \o <<Exit(Head(work).node)>> \o Tail(work)
    /\ UNCHANGED <<trace, credit, depth, phase>>

Charge ==
    /\ phase = "ready" /\ Len(work) > 0
    /\ Head(work).kind \in {"enter", "exit"}
    /\ credit > 0
    /\ trace' = Append(trace, Head(work))
    /\ credit' = credit - 1
    /\ work' = Tail(work)
    /\ IF Head(work).kind = "enter" /\ depth = MaxDepth
       THEN /\ phase' = "depth_rejected" /\ depth' = depth
       ELSE /\ phase' = "ready"
            /\ depth' = IF Head(work).kind = "enter"
                         THEN depth + 1 ELSE depth - 1

Deny ==
    /\ phase = "ready" /\ Len(work) > 0
    /\ Head(work).kind \in {"enter", "exit"}
    /\ credit = 0
    /\ phase' = "budget_rejected"
    /\ UNCHANGED <<work, trace, credit, depth>>

Finish ==
    /\ phase = "ready" /\ Len(work) = 0
    /\ phase' = "finished"
    /\ UNCHANGED <<work, trace, credit, depth>>

Terminal == phase \in {"finished", "budget_rejected", "depth_rejected"}
            /\ UNCHANGED vars
Next == Expand \/ Charge \/ Deny \/ Finish \/ Terminal
Spec == Init /\ [][Next]_vars

\* A source recursive DFS has exactly Expected; a PDA residual worklist has
\* exactly WorkTrace(work). Every admitted meter call is therefore the next
\* source event, even when a budget or depth failure stops the machine.
InvSourceOrder == trace \o WorkTrace(work) = Expected
InvPrefix == Len(trace) <= Len(Expected)
             /\ \A i \in 1..Len(trace) : trace[i] = Expected[i]
InvConservation == credit + Len(trace) = InitialCredit
InvDepth == depth >= 0 /\ depth <= MaxDepth
InvFinished == phase = "finished" => trace = Expected /\ depth = 0
InvRejected == phase \in {"budget_rejected", "depth_rejected"}
               => Len(trace) < Len(Expected)
==========================================================================
