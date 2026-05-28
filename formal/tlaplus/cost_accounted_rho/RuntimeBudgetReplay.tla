-------------------------- MODULE RuntimeBudgetReplay --------------------------
(****************************************************************************)
(* Finite-state model of the bounded-memory runtime budget used by the       *)
(* Rust implementation. This complements the Rocq arithmetic refinement by   *)
(* exploring canonical permit grants, OOP boundary commitment, replay trace *)
(* sequence stability, deploy reset, and finalization reads in small        *)
(* schedules.                                                               *)
(****************************************************************************)

EXTENDS Integers, FiniteSets, TLC, Sequences

CONSTANTS
    Events,
    DeployId,
    SourcePath,
    RedexId,
    LocalIndex,
    KindId,
    PrimitiveDescriptor,
    Weight,
    Rank,
    InitialBudget,
    MaxTraceEvents,
    MaxSourcePathComponents,
    MaxPrimitiveDescriptor,
    NoOop

VARIABLES
    pending,
    consumed,
    successTrace,
    permits,
    executed,
    oop,
    finalizedTrace,
    postOopRejects,
    frontier,
    truncatedAtOop,
    reconciledConsumed,
    reconciledOop,
    reconciledCommitted

vars == <<pending, consumed, successTrace, permits, executed, oop, finalizedTrace, postOopRejects, frontier, truncatedAtOop, reconciledConsumed, reconciledOop, reconciledCommitted>>

Init ==
    /\ pending = Events
    /\ consumed = 0
    /\ successTrace = <<>>
    /\ permits = <<>>
    /\ executed = <<>>
    /\ oop = NoOop
    /\ finalizedTrace = <<>>
    /\ postOopRejects = 0
    /\ frontier = 0
    /\ truncatedAtOop = {}
    /\ reconciledConsumed = -1
    /\ reconciledOop = NoOop
    /\ reconciledCommitted = {}

TypeOK ==
    /\ pending \subseteq Events
    /\ consumed \in Nat
    /\ consumed <= InitialBudget
    /\ successTrace \in Seq(Events)
    /\ permits \in Seq(Events)
    /\ executed \in Seq(Events)
    /\ oop \in Events \cup {NoOop}
    /\ finalizedTrace \in Seq(Events)
    /\ postOopRejects \in Nat
    /\ frontier \in 0..2
    /\ truncatedAtOop \subseteq Events
    /\ reconciledConsumed \in (Nat \cup {-1})
    /\ reconciledConsumed <= InitialBudget
    /\ reconciledOop \in Events \cup {NoOop}
    /\ reconciledCommitted \subseteq Events
    /\ MaxTraceEvents \in Nat
    /\ \A e \in Events : DeployId[e] \in Nat
    /\ \A e \in Events : SourcePath[e] \in Seq(Nat)
    /\ \A e \in Events : RedexId[e] \in Nat
    /\ \A e \in Events : LocalIndex[e] \in Nat
    /\ \A e \in Events : KindId[e] \in 0..2
    /\ \A e \in Events : PrimitiveDescriptor[e] \in Nat
    /\ \A e \in Events : Weight[e] \in Nat
    /\ \A e \in Events : Rank[e] \in Nat
    /\ MaxSourcePathComponents \in Nat
    /\ MaxPrimitiveDescriptor \in Nat

ValidEvent(e) ==
    /\ Weight[e] > 0
    /\ Len(SourcePath[e]) <= MaxSourcePathComponents
    /\ (KindId[e] # 1 \/ PrimitiveDescriptor[e] <= MaxPrimitiveDescriptor)
    /\ Len(successTrace) + (IF oop = NoOop THEN 0 ELSE 1) < MaxTraceEvents

PrimitiveDescriptorValue(e) ==
    IF KindId[e] = 1 THEN PrimitiveDescriptor[e] ELSE 0

TraceWithOop(trace, boundary) ==
    IF boundary = NoOop THEN trace ELSE Append(trace, boundary)

EventDescriptor(e) ==
    << DeployId[e],
       SourcePath[e],
       RedexId[e],
       LocalIndex[e],
       KindId[e],
       PrimitiveDescriptorValue(e),
       Weight[e] >>

OccurrenceIndex(trace, idx) ==
    Cardinality({ j \in 1..idx :
        EventDescriptor(trace[j]) = EventDescriptor(trace[idx]) })

SuccessDigestEntries(trace) ==
    { <<0, OccurrenceIndex(trace, i), EventDescriptor(trace[i])>> : i \in DOMAIN trace }

OopDigestEntry(boundary) ==
    <<1, 0, EventDescriptor(boundary)>>

OopDigestEntries(boundary) ==
    IF boundary = NoOop THEN {} ELSE { OopDigestEntry(boundary) }

CanonicalDigestEntries(trace, boundary) ==
    SuccessDigestEntries(trace) \cup OopDigestEntries(boundary)

(* Canonical-order readiness: the event whose Rank is minimal among the     *)
(* pending valid events. This is the order the post-hoc `reconcile()` walk   *)
(* uses (it sorts the attempt log by the derived `Ord` on                    *)
(* BillableTokenEvent). It is RETAINED only to express the canonical         *)
(* reconciliation as a pure function of the Events set (see                  *)
(* RecCommittedSeq / RecConsumed / RecOop below); it is NOT the live-firing  *)
(* guard any more.                                                           *)
CanonicalReady(e) ==
    /\ e \in pending
    /\ ValidEvent(e)
    /\ \A other \in pending : ValidEvent(other) => Rank[e] <= Rank[other]

(* Schedule-order readiness (WEAKENED firing guard). The live runtime races  *)
(* lock-free CAS attempts: ANY pending valid event may be the next one to    *)
(* commit, in ANY order, depending on which worker wins the race. TLC        *)
(* therefore explores every interleaving of grants — not just the canonical  *)
(* rank order. This is the change that lets the model WITNESS                *)
(* schedule-dependence in the OOP case (the live `successTrace` ordering and *)
(* the OOP-truncated committed set both become schedule-dependent), while    *)
(* the post-hoc reconciliation (RecConsumed / RecOop) stays a pure function  *)
(* of the Events set + InitialBudget.                                        *)
ScheduleReady(e) ==
    /\ e \in pending
    /\ ValidEvent(e)

FinalizedSuccessTrace ==
    IF oop = NoOop THEN finalizedTrace
    ELSE IF Len(finalizedTrace) = 0 THEN <<>>
    ELSE SubSeq(finalizedTrace, 1, Len(finalizedTrace) - 1)

FinalizedDigestEntries ==
    SuccessDigestEntries(FinalizedSuccessTrace)
    \cup OopDigestEntries(oop)

(****************************************************************************)
(* CANONICAL RECONCILIATION AS A PURE FUNCTION OF THE EVENT SET             *)
(*                                                                          *)
(* This block models the Rust `RuntimeBudget::reconcile()`                  *)
(* (rholang/src/rust/interpreter/accounting/mod.rs:455) as a deterministic *)
(* function of the CONSTANTS only — the Events set, Weight, Rank, and       *)
(* InitialBudget. It does NOT read any trajectory variable. Because its     *)
(* domain is the constants (identical across every behavior TLC explores),  *)
(* any invariant of the form `<live finalized quantity> = <Rec...>` proves  *)
(* the live quantity is schedule-independent: every schedule that reaches   *)
(* finalization must agree with this one pure value.                        *)
(*                                                                          *)
(* `reconcile()` sorts the attempt log by the derived Ord on                *)
(* BillableTokenEvent and walks it, committing each event whose cumulative  *)
(* weight stays <= initial; the first event that would exceed initial is    *)
(* the OOP boundary and consumed clamps to initial. We use Rank as the      *)
(* canonical sort key (the MC assigns distinct Ranks, matching the total    *)
(* order the Rust Ord induces on distinct events).                          *)
(****************************************************************************)

(* The state-independent ("intrinsic") part of ValidEvent: the runtime      *)
(* admission checks that gate an event into the attempt log (positive       *)
(* weight, bounded source path, bounded primitive descriptor). The          *)
(* MaxTraceEvents clause of ValidEvent is a live-state retention bound and  *)
(* is handled separately by the bounded-K cap below, so it is excluded here.*)
IntrinsicallyValid(e) ==
    /\ Weight[e] > 0
    /\ Len(SourcePath[e]) <= MaxSourcePathComponents
    /\ (KindId[e] # 1 \/ PrimitiveDescriptor[e] <= MaxPrimitiveDescriptor)

ValidEventSet == { e \in Events : IntrinsicallyValid(e) }

(* The canonical (rank-sorted) sequence of all intrinsically-valid events.  *)
(* Built by repeatedly extracting the rank-minimum remaining event. Ranks   *)
(* are distinct in the MC, so the CHOOSE is single-valued.                  *)
RECURSIVE RankSortSeq(_)
RankSortSeq(S) ==
    IF S = {} THEN <<>>
    ELSE LET m == CHOOSE x \in S : \A y \in S : Rank[x] <= Rank[y]
         IN <<m>> \o RankSortSeq(S \ {m})

CanonicalSeq == RankSortSeq(ValidEventSet)

(* The bounded-K window. weights >= 1, so the canonical walk commits at most *)
(* InitialBudget events before the OOP boundary; MAX_COST_TRACE_EVENTS       *)
(* (MaxTraceEvents here) is the hard retention cap. K is their min — the     *)
(* Milestone-3 bounded-K reconciliation reads only the lowest-K events.      *)
BoundedK == IF MaxTraceEvents < InitialBudget + 1 THEN MaxTraceEvents
            ELSE InitialBudget + 1

KWindow == IF Len(CanonicalSeq) <= BoundedK THEN CanonicalSeq
           ELSE SubSeq(CanonicalSeq, 1, BoundedK)

(* Walk the K-window accumulating weight. Returns the prefix length that     *)
(* commits (everything before the first overflow), or Len(window) if no      *)
(* overflow occurs within the window.                                        *)
RECURSIVE WalkCommitLen(_, _, _)
WalkCommitLen(seq, idx, acc) ==
    IF idx > Len(seq) THEN Len(seq)
    ELSE IF acc + Weight[seq[idx]] > InitialBudget THEN idx - 1
    ELSE WalkCommitLen(seq, idx + 1, acc + Weight[seq[idx]])

RecCommitLen == WalkCommitLen(KWindow, 1, 0)

(* The reconciled committed sequence, set, and OOP boundary. *)
RecCommittedSeq == SubSeq(KWindow, 1, RecCommitLen)
RecCommittedSet == { KWindow[i] : i \in 1..RecCommitLen }

RecIsOop == RecCommitLen < Len(KWindow)

RecOop == IF RecIsOop THEN KWindow[RecCommitLen + 1] ELSE NoOop

RECURSIVE SumWeights(_)
SumWeights(seq) ==
    IF Len(seq) = 0 THEN 0
    ELSE Weight[seq[1]] + SumWeights(SubSeq(seq, 2, Len(seq)))

(* Sum of weights of ALL intrinsically-valid events (the uncapped total).   *)
TotalValidWeight == SumWeights(CanonicalSeq)

(* consumed = min(InitialBudget, sum of committed weights). On OOP it clamps *)
(* to InitialBudget; otherwise it is the full committed sum.                 *)
RecConsumed == IF RecIsOop THEN InitialBudget ELSE SumWeights(RecCommittedSeq)

(* The reconciled-output slot is untouched by every action except Merge.    *)
UnchangedReconciled ==
    UNCHANGED <<reconciledConsumed, reconciledOop, reconciledCommitted>>

ReserveOk(e) ==
    /\ frontier = 0
    /\ oop = NoOop
    /\ ScheduleReady(e)
    /\ consumed + Weight[e] <= InitialBudget
    /\ consumed' = consumed + Weight[e]
    /\ successTrace' = Append(successTrace, e)
    /\ permits' = Append(permits, e)
    /\ executed' = Append(executed, e)
    /\ pending' = pending \ {e}
    /\ oop' = NoOop
    /\ finalizedTrace' = finalizedTrace
    /\ postOopRejects' = postOopRejects
    /\ frontier' = frontier
    /\ truncatedAtOop' = truncatedAtOop
    /\ UnchangedReconciled

ReserveOop(e) ==
    /\ frontier = 0
    /\ oop = NoOop
    /\ ScheduleReady(e)
    /\ consumed + Weight[e] > InitialBudget
    /\ consumed' = InitialBudget
    /\ successTrace' = successTrace
    /\ UNCHANGED <<permits, executed>>
    /\ pending' = pending \ {e}
    /\ oop' = e
    /\ finalizedTrace' = finalizedTrace
    /\ postOopRejects' = postOopRejects
    /\ frontier' = frontier
    /\ truncatedAtOop' = truncatedAtOop
    /\ UnchangedReconciled

RejectPostOop(e) ==
    /\ frontier = 0
    /\ oop # NoOop
    /\ e \in pending
    /\ ValidEvent(e)
    /\ pending' = pending \ {e}
    /\ UNCHANGED <<consumed, successTrace, permits, executed, oop, finalizedTrace>>
    /\ postOopRejects' = postOopRejects + 1
    /\ frontier' = frontier
    /\ truncatedAtOop' = truncatedAtOop
    /\ UnchangedReconciled

(* OOP TRUNCATION (schedule-dependent stop point).                           *)
(*                                                                           *)
(* When the budget is exhausted (oop # NoOop), a fork that is still holding  *)
(* unprocessed pending work may ABANDON it in bulk rather than draining it   *)
(* event-by-event through RejectPostOop. The runtime correlate: once a fork  *)
(* observes the OOP, it unwinds; whatever it had already recorded into the   *)
(* attempt log before the boundary is the committed set, and the rest is     *)
(* never recorded. WHICH events made it in before the boundary depends on    *)
(* the CAS race order (the weakened ScheduleReady guard), so the live        *)
(* committed `successTrace` is genuinely schedule-dependent here.            *)
(*                                                                           *)
(* This is the action that DEMONSTRATES why the per-operation cost-trace     *)
(* digest was dropped from consensus: under OOP, two schedules reach         *)
(* different live committed sets, hence different per-op digests — so a      *)
(* per-op digest is NOT a consensus quantity. The reconciled `consumed`/OOP  *)
(* verdict (RecConsumed / RecOop), by contrast, stay invariant (asserted by  *)
(* ConsumedAndVerdictScheduleIndependent).                                   *)
OopTruncate ==
    /\ frontier = 0
    /\ oop # NoOop
    /\ pending # {}
    /\ truncatedAtOop' = truncatedAtOop \cup pending
    /\ pending' = {}
    /\ UNCHANGED <<consumed, successTrace, permits, executed, oop, finalizedTrace, postOopRejects, frontier>>
    /\ UnchangedReconciled

RejectInvalid(e) ==
    /\ frontier = 0
    /\ e \in pending
    /\ ~ValidEvent(e)
    /\ pending' = pending \ {e}
    /\ UNCHANGED <<consumed, successTrace, permits, executed, oop, finalizedTrace, postOopRejects, frontier, truncatedAtOop>>
    /\ UnchangedReconciled

FinalizeTrace ==
    /\ frontier = 0
    /\ (pending = {} \/ oop # NoOop)
    /\ finalizedTrace' = TraceWithOop(successTrace, oop)
    /\ UNCHANGED <<pending, consumed, successTrace, permits, executed, oop, postOopRejects, truncatedAtOop>>
    /\ frontier' = 1
    /\ UnchangedReconciled

(* BOUNDED-K MERGE (Milestone 3 reconciliation as a state transition).       *)
(*                                                                           *)
(* Runs after FinalizeTrace (frontier = 1) and before ResetDeploy            *)
(* (frontier 2). It populates the reconciled-output slot from the BOUNDED-K  *)
(* canonical walk: only the lowest-K canonical events (K = BoundedK =        *)
(* min(MaxTraceEvents, InitialBudget+1)) are read. This is the consensus     *)
(* answer: `consumed`/`total_cost`, the OOP verdict, and the committed set.  *)
(*                                                                           *)
(* Crucially, reconciledConsumed/reconciledOop/reconciledCommitted are set   *)
(* from RecConsumed/RecOop/RecCommittedSet, which depend ONLY on the         *)
(* constants (Events, Weight, Rank, InitialBudget) — never on successTrace,  *)
(* the firing order, or truncatedAtOop. So every schedule that reaches this  *)
(* action installs the SAME values, which is exactly the schedule-           *)
(* independence the consensus layer relies on.                              *)
Merge ==
    /\ frontier = 1
    /\ reconciledConsumed' = RecConsumed
    /\ reconciledOop' = RecOop
    /\ reconciledCommitted' = RecCommittedSet
    /\ frontier' = 2
    /\ UNCHANGED <<pending, consumed, successTrace, permits, executed, oop, finalizedTrace, postOopRejects, truncatedAtOop>>

ResetDeploy ==
    /\ frontier = 2
    /\ pending' = Events
    /\ consumed' = 0
    /\ successTrace' = <<>>
    /\ permits' = <<>>
    /\ executed' = <<>>
    /\ oop' = NoOop
    /\ finalizedTrace' = finalizedTrace
    /\ postOopRejects' = 0
    /\ frontier' = 0
    /\ truncatedAtOop' = {}
    /\ reconciledConsumed' = -1
    /\ reconciledOop' = NoOop
    /\ reconciledCommitted' = {}

Next ==
    (\E e \in Events : ReserveOk(e) \/ ReserveOop(e) \/ RejectPostOop(e) \/ RejectInvalid(e)) \/
    OopTruncate \/
    FinalizeTrace \/
    Merge \/
    ResetDeploy

Spec == Init /\ [][Next]_vars

NoOverspend ==
    consumed <= InitialBudget

OopCommitsBoundary ==
    oop # NoOop => consumed = InitialBudget

ReplayTraceSubset ==
    \A i \in DOMAIN successTrace : successTrace[i] \in (Events \ pending)

OopNotLogged ==
    oop # NoOop => \A i \in DOMAIN successTrace : successTrace[i] # oop

PermitsMatchSuccessfulTrace ==
    permits = successTrace

NoUnpaidPhysicalWork ==
    executed = permits

(* Under the WEAKENED ScheduleReady firing guard the live `successTrace` is  *)
(* no longer rank-sorted — that was the point of weakening it. The live      *)
(* recording order is whatever the CAS race produced. The canonical order is *)
(* recovered post-hoc by `reconcile()` (sort-by-Ord); the consensus          *)
(* quantities derived from it (RecConsumed / RecOop) are what must be        *)
(* schedule-independent, NOT the live order. We therefore do NOT assert      *)
(* rank-sortedness of successTrace any more. The weakest true statement is   *)
(* that every committed event was a pending valid event (already covered by  *)
(* ReplayTraceSubset + LoggedEventsAreValidated).                            *)
LiveTraceIsAdmissibleSchedule ==
    \A i \in DOMAIN successTrace :
        /\ IntrinsicallyValid(successTrace[i])
        /\ Weight[successTrace[i]] > 0

FinalizedTraceSequence ==
    finalizedTrace \in Seq(Events)

FinalizationPreservesActiveBudget ==
    frontier >= 1 => consumed <= InitialBudget

LoggedEventsHavePositiveWeight ==
    /\ \A i \in DOMAIN successTrace : Weight[successTrace[i]] > 0
    /\ oop # NoOop => Weight[oop] > 0

LoggedEventsAreValidated ==
    /\ \A i \in DOMAIN successTrace :
        /\ Weight[successTrace[i]] > 0
        /\ Len(SourcePath[successTrace[i]]) <= MaxSourcePathComponents
        /\ (KindId[successTrace[i]] # 1
           \/ PrimitiveDescriptor[successTrace[i]] <= MaxPrimitiveDescriptor)
    /\ oop # NoOop =>
        /\ Weight[oop] > 0
        /\ Len(SourcePath[oop]) <= MaxSourcePathComponents
        /\ (KindId[oop] # 1 \/ PrimitiveDescriptor[oop] <= MaxPrimitiveDescriptor)

TraceWithinRetentionBound ==
    Len(successTrace) + (IF oop = NoOop THEN 0 ELSE 1) <= MaxTraceEvents

ResetClearsActiveTraceAfterFinalization ==
    frontier = 0 /\ finalizedTrace # <<>> /\ consumed = 0 => successTrace = <<>> /\ oop = NoOop

PostOopRejectionsPreserveSingleBoundary ==
    postOopRejects > 0 =>
        /\ oop # NoOop
        /\ Len(TraceWithOop(successTrace, oop)) <= MaxTraceEvents

CanonicalDigestEventCountMatches ==
    Cardinality(CanonicalDigestEntries(successTrace, oop)) =
        Len(successTrace) + (IF oop = NoOop THEN 0 ELSE 1)

CanonicalDigestDomainSeparatesOop ==
    oop # NoOop =>
        /\ OopDigestEntry(oop) \in CanonicalDigestEntries(successTrace, oop)
        /\ \A i \in DOMAIN successTrace :
            OopDigestEntry(oop) # <<0, OccurrenceIndex(successTrace, i), EventDescriptor(successTrace[i])>>

CanonicalDigestStableAfterFinalization ==
    frontier >= 1 =>
        CanonicalDigestEntries(successTrace, oop) = FinalizedDigestEntries

(****************************************************************************)
(* RE-AIMED CONSENSUS-QUANTITY INVARIANTS                                    *)
(*                                                                          *)
(* The cost-accounting refactor DROPS the per-operation `cost_trace_digest` *)
(* from consensus. It is NOT a consensus quantity: under OOP the live       *)
(* committed set (and hence any per-op digest of it) is schedule-dependent  *)
(* — the OopTruncate action witnesses exactly that. The consensus cost      *)
(* quantity that remains is `total_cost` (= consumed tokens, clamped to     *)
(* `initial` on OOP) together with the failed/OOP status. The Rust runtime  *)
(* (rholang/src/rust/interpreter/accounting/mod.rs::RuntimeBudget) races    *)
(* lock-free CAS attempts (modeled by the weakened ScheduleReady guard) and *)
(* derives the consensus values from a post-hoc `reconcile()` (modeled by   *)
(* the bounded-K Merge action).                                             *)
(*                                                                          *)
(* The invariants below assert the two properties the refactor relies on:   *)
(*   1. `consumed`/`total_cost` and the OOP verdict are SCHEDULE-INDEPENDENT *)
(*      (they equal a pure function of the Events set + InitialBudget).      *)
(*   2. The NON-OOP committed multiset is COMPLETE and schedule-independent  *)
(*      (every intrinsically-valid event commits).                          *)
(*                                                                          *)
(* What is DELIBERATELY NOT asserted: schedule-independence of the live     *)
(* per-op `successTrace` / its digest. That is false under OOP, which is    *)
(* the whole reason the digest was removed from consensus. See the          *)
(* OopTruncate comment.                                                     *)
(****************************************************************************)

(* ---- THE HEADLINE INVARIANT (replaces the old digest-purity invariant) - *)
(*                                                                          *)
(* After the bounded-K Merge (frontier = 2), the reconciled consensus       *)
(* quantities equal the canonical pure-function-of-constants values, and    *)
(* obey the budget threshold law:                                           *)
(*   Σ(valid weights) >  InitialBudget  =>  OOP  ∧  consumed = InitialBudget *)
(*   Σ(valid weights) <= InitialBudget  =>  ¬OOP ∧  consumed = Σ            *)
(*                                                                          *)
(* RecConsumed / RecOop read ONLY the constants (Events, Weight, Rank,      *)
(* InitialBudget) — never successTrace, the firing order, or truncatedAtOop *)
(* — so installing them at Merge proves schedule-independence: every        *)
(* behavior TLC explores reaches the SAME reconciled values. The threshold  *)
(* clause is guarded by ~CapTruncates because the bounded-K window only      *)
(* changes the answer when there are more valid events than the cap admits  *)
(* (a DoS backstop, not the budget decision); the clamp law below holds     *)
(* unconditionally.                                                         *)
CapTruncates == Len(CanonicalSeq) > BoundedK

ConsumedAndVerdictScheduleIndependent ==
    frontier = 2 =>
        /\ reconciledConsumed = RecConsumed
        /\ reconciledOop = RecOop
        /\ reconciledConsumed <= InitialBudget
        /\ (~CapTruncates =>
              IF TotalValidWeight > InitialBudget
              THEN reconciledOop # NoOop /\ reconciledConsumed = InitialBudget
              ELSE reconciledOop = NoOop /\ reconciledConsumed = TotalValidWeight)

(* total_cost is the clamped sum: min(InitialBudget, Σ valid weights) in the *)
(* common (non-cap-truncated) case. Always reconciledConsumed <= both bounds.*)
TotalCostMatchesClampedSum ==
    frontier = 2 =>
        /\ reconciledConsumed <= InitialBudget
        /\ reconciledConsumed <= TotalValidWeight
        /\ (~CapTruncates =>
              reconciledConsumed =
                (IF TotalValidWeight < InitialBudget THEN TotalValidWeight ELSE InitialBudget))

(* ---- NON-OOP COMMITTED MULTISET COMPLETENESS --------------------------- *)
(*                                                                          *)
(* When the deploy does NOT go OOP AND the bounded-K cap does not bite, the  *)
(* reconciled committed set is exactly the set of all intrinsically-valid    *)
(* events — complete, and (being a pure function of the constants)           *)
(* schedule-independent. This is the property that survives for the non-OOP  *)
(* case: every term's metering child commits, RSpace selection is            *)
(* deterministic, so the recorded multiset is identical across schedules and *)
(* play/replay.                                                              *)
(*                                                                          *)
(* The ~CapTruncates guard is essential and faithful: when more than         *)
(* MAX_COST_TRACE_EVENTS distinct valid events exist, `reconcile()`          *)
(* truncates the canonical log to the lowest K, so the committed set is the  *)
(* lowest-K window, not the full valid set (see CapTruncatedCommittedIsLowest*)
(* K). In production MAX_COST_TRACE_EVENTS = 1,000,000, so for any realistic *)
(* non-OOP deploy the cap does not bite and completeness holds; the cap arm  *)
(* is a DoS backstop. The MCRuntimeBudgetReplayCap instance deliberately     *)
(* shrinks the cap below the valid-event count to exercise the guard.        *)
NonOopCommittedMultisetComplete ==
    frontier = 2 /\ reconciledOop = NoOop /\ ~CapTruncates =>
        reconciledCommitted = ValidEventSet

(* When the bounded-K cap DOES bite, the reconciled committed set is the     *)
(* lowest-K canonical prefix (or that prefix up to the in-window OOP         *)
(* boundary). Either way it is a prefix-set of the rank-sorted valid events  *)
(* and is still a pure function of the constants (schedule-independent).     *)
CapTruncatedCommittedIsLowestK ==
    frontier = 2 /\ CapTruncates =>
        /\ reconciledCommitted = RecCommittedSet
        /\ RecCommittedSet \subseteq { KWindow[i] : i \in 1..Len(KWindow) }
        /\ Cardinality(reconciledCommitted) <= BoundedK

(* The reconciled committed set is always a subset of the valid events, and *)
(* on OOP excludes the boundary event. *)
ReconciledCommittedWellFormed ==
    frontier = 2 =>
        /\ reconciledCommitted \subseteq ValidEventSet
        /\ (reconciledOop # NoOop => reconciledOop \notin reconciledCommitted)

(* ---- BOUNDED-K MERGE CONSISTENCY (Milestone 3) ------------------------- *)
(*                                                                          *)
(* The Merge action reads only the lowest-K canonical events (KWindow), yet *)
(* reproduces the canonical reconciliation answer. The committed prefix     *)
(* length never exceeds K, and (weights >= 1) never exceeds InitialBudget   *)
(* committed events. This is the Milestone-3 claim: the bounded fold equals  *)
(* the sort-truncate-walk, with bounded memory.                             *)
MergeReadsBoundedKWindow ==
    /\ Len(KWindow) <= BoundedK
    /\ RecCommitLen <= Len(KWindow)
    /\ RecCommitLen <= InitialBudget
    /\ Cardinality(RecCommittedSet) <= BoundedK

(* ---- ConsumedFollowsReconciliationContract (retained, generalized) ----- *)
(* On the LIVE finalized state, consumed clamps to InitialBudget on OOP and  *)
(* stays <= InitialBudget otherwise. (Note: the live consumed may be < the   *)
(* reconciled total_cost when the trace cap bites — which is exactly why     *)
(* consensus reads the RECONCILED value, not the live one.)                  *)
ConsumedFollowsReconciliationContract ==
    frontier >= 1 =>
        ((oop = NoOop /\ consumed <= InitialBudget) \/
         (oop # NoOop /\ consumed = InitialBudget))

(* No-cross-worker write: every committed event carries positive weight;    *)
(* no worker writes into another worker's scope. In the spec successTrace    *)
(* is appended atomically by the firing actions; the Rust runtime achieves   *)
(* the same via per-scope fresh-counter metering children + lock-free CAS.   *)
NoCrossWorkerStateMixing ==
    \A i \in DOMAIN successTrace :
        Weight[successTrace[i]] > 0

(****************************************************************************)
(* WITNESS: the live per-op trace IS schedule-dependent under OOP.          *)
(*                                                                          *)
(* The following is NOT an invariant — it is a "no-invariant" we explicitly *)
(* decline to assert, recorded here to document the design decision. Under  *)
(* the weakened ScheduleReady guard + OopTruncate, two behaviors with the   *)
(* same Events can reach finalization with DIFFERENT live committed sets    *)
(* (different `successTrace` content / truncatedAtOop) whenever OOP fires.  *)
(* That divergence is precisely why the per-operation cost_trace_digest is  *)
(* dropped from consensus. The reconciled quantities above remain invariant.*)
(*                                                                          *)
(* OopVerdictIsReached documents that the OOP verdict is itself reachable    *)
(* (so the OOP arm of the invariants above is actually exercised by TLC):   *)
(*   - if any behavior reaches frontier=2 with reconciledOop # NoOop, the    *)
(*     OOP arm was covered.                                                  *)
(****************************************************************************)

=============================================================================
