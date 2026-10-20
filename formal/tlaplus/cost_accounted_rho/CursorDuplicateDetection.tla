-------------------- MODULE CursorDuplicateDetection --------------------
(***************************************************************************)
(* DR-120 (gap G9), law L7: the fast path of the v6 merge never composes a *)
(* scope copy of a deploy whose settlement the base holds.                 *)
(*                                                                         *)
(* A history is a common part, then the settlements of the base side and  *)
(* of the scope side after the divergence. A settlement that moves the     *)
(* cohort's fee cursor consumes the datum of the current revision and      *)
(* produces the next one. The fast path composes the scope side only when  *)
(* its first cursor settlement consumes the revision that the base holds   *)
(* (P6, dev's availability walk). The slow path drops every scope copy of  *)
(* a deploy that the base holds (dev's settled-sig dedup).                 *)
(*                                                                         *)
(* Controls: a mergeable revision counter consumes no datum; settlements  *)
(* without a fee transition move no cursor; without the repeat rule a      *)
(* block may settle a deploy that its own history settled.                 *)
(***************************************************************************)
EXTENDS Naturals, Sequences, FiniteSets

CONSTANTS Deploys, MaxSide, MergeableRevision, FeeTransitions, RepeatRule
ASSUME /\ IsFiniteSet(Deploys) /\ Deploys # {}
       /\ MaxSide \in Nat
       /\ MergeableRevision \in BOOLEAN
       /\ FeeTransitions \in BOOLEAN
       /\ RepeatRule \in BOOLEAN

VARIABLES common, baseSide, scopeSide, diverged
vars == <<common, baseSide, scopeSide, diverged>>

\* A settlement: its deploy, and whether it moves the cohort's fee cursor.
Settlements == [deploy : Deploys, cursor : BOOLEAN]
CursorFlags == IF FeeTransitions THEN {TRUE} ELSE BOOLEAN

DeploysOf(line) == {line[i].deploy : i \in 1..Len(line)}
Moves(line) == Cardinality({i \in 1..Len(line) : line[i].cursor})

Init == /\ common = <<>>
        /\ baseSide = <<>>
        /\ scopeSide = <<>>
        /\ diverged = FALSE

\* The repeat rule: a block never settles a deploy that its own history
\* settled (validate.rs:601-717).
Allowed(d, history) == ~RepeatRule \/ d \notin DeploysOf(history)

AddCommon(d, c) ==
    /\ ~diverged
    /\ Len(common) < MaxSide
    /\ Allowed(d, common)
    /\ common' = Append(common, [deploy |-> d, cursor |-> c])
    /\ UNCHANGED <<baseSide, scopeSide, diverged>>

Diverge ==
    /\ ~diverged
    /\ diverged' = TRUE
    /\ UNCHANGED <<common, baseSide, scopeSide>>

AddBase(d, c) ==
    /\ diverged
    /\ Len(baseSide) < MaxSide
    /\ Allowed(d, common \o baseSide)
    /\ baseSide' = Append(baseSide, [deploy |-> d, cursor |-> c])
    /\ UNCHANGED <<common, scopeSide, diverged>>

AddScope(d, c) ==
    /\ diverged
    /\ Len(scopeSide) < MaxSide
    /\ Allowed(d, common \o scopeSide)
    /\ scopeSide' = Append(scopeSide, [deploy |-> d, cursor |-> c])
    /\ UNCHANGED <<common, baseSide, diverged>>

Next == \/ \E d \in Deploys, c \in CursorFlags :
             AddCommon(d, c) \/ AddBase(d, c) \/ AddScope(d, c)
        \/ Diverge
Spec == Init /\ [][Next]_vars

\* The revision that the base holds, and the revision that the first cursor
\* settlement of the scope side consumes.
BaseRevision == Moves(common) + Moves(baseSide)
ScopeFirstConsumes == Moves(common)

\* P6 on the cursor. A mergeable counter consumes no datum, so the check
\* passes whatever the base holds.
FastPath ==
    \/ MergeableRevision
    \/ Moves(scopeSide) = 0
    \/ ScopeFirstConsumes = BaseRevision

\* The base holds a settlement of a deploy that the scope side settles again.
Duplicate == (DeploysOf(common \o baseSide) \cap DeploysOf(scopeSide)) # {}

\* The composed outcome: every settlement on the fast path; on the slow
\* path, the base and the first scope copy of each deploy that the base does
\* not hold.
FirstCopies(line, held) ==
    LET keep(i) == /\ line[i].deploy \notin held
                   /\ \A j \in 1..(i - 1) : line[j].deploy # line[i].deploy
    IN SelectSeq([i \in 1..Len(line) |-> i], keep)
Outcome ==
    IF FastPath
    THEN common \o baseSide \o scopeSide
    ELSE LET kept == FirstCopies(scopeSide, DeploysOf(common \o baseSide))
         IN common \o baseSide \o [k \in 1..Len(kept) |-> scopeSide[kept[k]]]
Charges(d) == Cardinality({i \in 1..Len(Outcome) : Outcome[i].deploy = d})

DuplicateNeverFastPath == Duplicate => ~FastPath
ExactlyOnceCharge == \A d \in Deploys : Charges(d) <= 1
TypeOK == /\ common \in Seq(Settlements)
          /\ baseSide \in Seq(Settlements)
          /\ scopeSide \in Seq(Settlements)
          /\ diverged \in BOOLEAN

=====================================================================
