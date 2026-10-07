-------------------- MODULE NativeOperationBudgetOrder --------------------
EXTENDS Naturals, FiniteSets, Sequences

CONSTANTS CheckCuts, ForgeCuts, CheckDependencies
Operations == {1, 2, 3}
Roots == {1, 2}
Limit == 2
Parent(w) == IF w = 3 THEN {1} ELSE {}

VARIABLES phase, started, completed, played, starts, ends, candidate, used,
          granted, accepted, proposedStarts, proposedEnds
vars == <<phase, started, completed, played, starts, ends, candidate, used,
          granted, accepted, proposedStarts, proposedEnds>>
Members(sequence) == {sequence[i] : i \in 1..Len(sequence)}
Position(w) == CHOOSE i \in 1..Len(candidate) : candidate[i] = w

Init ==
  /\ phase = "Play"
  /\ started = {}
  /\ completed = {}
  /\ played = <<>>
  /\ starts = [w \in Operations |-> 0]
  /\ ends = [w \in Operations |-> 0]
  /\ candidate = <<>>
  /\ used = 0
  /\ granted = {}
  /\ accepted = FALSE
  /\ proposedStarts = [w \in Operations |-> 0]
  /\ proposedEnds = [w \in Operations |-> 0]

Start(w) ==
  /\ phase = "Play"
  /\ w \notin started
  /\ w \in Roots \/ Roots \subseteq completed
  /\ started' = started \cup {w}
  /\ starts' = [starts EXCEPT ![w] = Len(played)]
  /\ UNCHANGED <<phase, completed, played, ends, candidate, used, granted, accepted,
                  proposedStarts, proposedEnds>>

Charge(w) ==
  /\ phase = "Play"
  /\ w \in started \ Members(played)
  /\ played' = Append(played, w)
  /\ UNCHANGED <<phase, started, completed, starts, ends, candidate, used, granted, accepted,
                  proposedStarts, proposedEnds>>

Complete(w) ==
  /\ phase = "Play"
  /\ w \in Members(played) \ completed
  /\ completed' = completed \cup {w}
  /\ ends' = [ends EXCEPT ![w] = Len(played)]
  /\ UNCHANGED <<phase, started, played, starts, candidate, used, granted, accepted,
                  proposedStarts, proposedEnds>>

Seal ==
  /\ phase = "Play"
  /\ completed = Operations
  /\ phase' = "Audit"
  /\ UNCHANGED <<started, completed, played, starts, ends, candidate, used, granted, accepted,
                  proposedStarts, proposedEnds>>

ProposeRow(w) ==
  /\ phase = "Audit"
  /\ w \notin Members(candidate)
  /\ candidate' = Append(candidate, w)
  /\ IF ForgeCuts
     THEN \E lo, hi \in 0..3 :
            /\ lo <= hi
            /\ proposedStarts' = [proposedStarts EXCEPT ![w] = lo]
            /\ proposedEnds' = [proposedEnds EXCEPT ![w] = hi]
     ELSE UNCHANGED <<proposedStarts, proposedEnds>>
  /\ IF used < Limit
     THEN /\ used' = used + 1
          /\ granted' = granted \cup {w}
     ELSE UNCHANGED <<used, granted>>
  /\ UNCHANGED <<phase, started, completed, played, starts, ends, accepted>>

AuditStart(w) == IF ForgeCuts THEN proposedStarts[w] ELSE starts[w]
AuditEnd(w) == IF ForgeCuts THEN proposedEnds[w] ELSE ends[w]
CutsMatch == \A w \in Operations : AuditStart(w) < Position(w) /\ Position(w) <= AuditEnd(w)
DependencyCuts == \A root \in Roots : AuditEnd(root) <= AuditStart(3)
ParentsMatched == \A w \in Operations : Parent(w) \subseteq granted
Audit ==
  /\ phase = "Audit"
  /\ Members(candidate) = Operations
  /\ accepted' = ParentsMatched /\ (~CheckCuts \/ CutsMatch)
                   /\ (~CheckDependencies \/ DependencyCuts)
  /\ phase' = "Done"
  /\ UNCHANGED <<started, completed, played, starts, ends, candidate, used, granted,
                  proposedStarts, proposedEnds>>

Next == Seal \/ Audit \/
  (\E w \in Operations : Start(w) \/ Charge(w) \/ Complete(w) \/ ProposeRow(w))
Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ phase \in {"Play", "Audit", "Done"}
  /\ started \subseteq Operations
  /\ completed \subseteq started
  /\ played \in Seq(Operations)
  /\ candidate \in Seq(Operations)
  /\ Len(played) = Cardinality(Members(played))
  /\ Len(candidate) = Cardinality(Members(candidate))
  /\ starts \in [Operations -> 0..3]
  /\ ends \in [Operations -> 0..3]
  /\ proposedStarts \in [Operations -> 0..3]
  /\ proposedEnds \in [Operations -> 0..3]
  /\ granted \subseteq Members(candidate)
  /\ used \in 0..Limit
  /\ used = Cardinality(granted)
  /\ accepted \in BOOLEAN

CausalBudgetOrder == accepted =>
  \A w \in Operations : \A parent \in Parent(w) : Position(parent) < Position(w)
FrontierBudgetOrder == accepted => \A root \in Roots : Position(root) < Position(3)
IndependentRootEnabled == phase = "Play" =>
  \A root \in Roots \ started : ENABLED Start(root)
NoFixedRootOrder == accepted => Position(1) < Position(2)
=============================================================================
