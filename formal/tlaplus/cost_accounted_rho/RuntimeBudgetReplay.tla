-------------------------- MODULE RuntimeBudgetReplay --------------------------
(****************************************************************************)
(* Finite-state model of the bounded-memory runtime budget used by the       *)
(* Rust implementation. This complements the Rocq arithmetic refinement by   *)
(* exploring reservation interleavings, OOP boundary commitment, replay      *)
(* trace sequence stability, deploy reset, and finalization reads in small   *)
(* schedules.                                                               *)
(****************************************************************************)

EXTENDS Naturals, FiniteSets, TLC, Sequences

CONSTANTS
    Events,
    DeployId,
    SourcePath,
    RedexId,
    LocalIndex,
    KindId,
    PrimitiveDescriptor,
    Weight,
    InitialBudget,
    MaxTraceEvents,
    MaxSourcePathComponents,
    MaxPrimitiveDescriptor,
    NoOop

VARIABLES
    pending,
    consumed,
    successTrace,
    oop,
    finalizedTrace,
    postOopRejects,
    frontier

vars == <<pending, consumed, successTrace, oop, finalizedTrace, postOopRejects, frontier>>

Init ==
    /\ pending = Events
    /\ consumed = 0
    /\ successTrace = <<>>
    /\ oop = NoOop
    /\ finalizedTrace = <<>>
    /\ postOopRejects = 0
    /\ frontier = 0

TypeOK ==
    /\ pending \subseteq Events
    /\ consumed \in Nat
    /\ consumed <= InitialBudget
    /\ successTrace \in Seq(Events)
    /\ oop \in Events \cup {NoOop}
    /\ finalizedTrace \in Seq(Events)
    /\ postOopRejects \in Nat
    /\ frontier \in 0..1
    /\ MaxTraceEvents \in Nat
    /\ \A e \in Events : DeployId[e] \in Nat
    /\ \A e \in Events : SourcePath[e] \in Seq(Nat)
    /\ \A e \in Events : RedexId[e] \in Nat
    /\ \A e \in Events : LocalIndex[e] \in Nat
    /\ \A e \in Events : KindId[e] \in 0..2
    /\ \A e \in Events : PrimitiveDescriptor[e] \in Nat
    /\ \A e \in Events : Weight[e] \in Nat
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

FinalizedSuccessTrace ==
    IF oop = NoOop THEN finalizedTrace
    ELSE IF Len(finalizedTrace) = 0 THEN <<>>
    ELSE SubSeq(finalizedTrace, 1, Len(finalizedTrace) - 1)

FinalizedDigestEntries ==
    SuccessDigestEntries(FinalizedSuccessTrace)
    \cup OopDigestEntries(oop)

ReserveOk(e) ==
    /\ frontier = 0
    /\ oop = NoOop
    /\ e \in pending
    /\ ValidEvent(e)
    /\ consumed + Weight[e] <= InitialBudget
    /\ consumed' = consumed + Weight[e]
    /\ successTrace' = Append(successTrace, e)
    /\ pending' = pending \ {e}
    /\ oop' = NoOop
    /\ finalizedTrace' = finalizedTrace
    /\ postOopRejects' = postOopRejects
    /\ frontier' = frontier

ReserveOop(e) ==
    /\ frontier = 0
    /\ oop = NoOop
    /\ e \in pending
    /\ ValidEvent(e)
    /\ consumed + Weight[e] > InitialBudget
    /\ consumed' = InitialBudget
    /\ successTrace' = successTrace
    /\ pending' = pending \ {e}
    /\ oop' = e
    /\ finalizedTrace' = finalizedTrace
    /\ postOopRejects' = postOopRejects
    /\ frontier' = frontier

RejectPostOop(e) ==
    /\ frontier = 0
    /\ oop # NoOop
    /\ e \in pending
    /\ ValidEvent(e)
    /\ pending' = pending \ {e}
    /\ UNCHANGED <<consumed, successTrace, oop, finalizedTrace>>
    /\ postOopRejects' = postOopRejects + 1
    /\ frontier' = frontier

RejectInvalid(e) ==
    /\ frontier = 0
    /\ e \in pending
    /\ ~ValidEvent(e)
    /\ pending' = pending \ {e}
    /\ UNCHANGED <<consumed, successTrace, oop, finalizedTrace, postOopRejects, frontier>>

FinalizeTrace ==
    /\ frontier = 0
    /\ (pending = {} \/ oop # NoOop)
    /\ finalizedTrace' = TraceWithOop(successTrace, oop)
    /\ UNCHANGED <<pending, consumed, successTrace, oop, postOopRejects>>
    /\ frontier' = 1

ResetDeploy ==
    /\ frontier = 1
    /\ pending' = Events
    /\ consumed' = 0
    /\ successTrace' = <<>>
    /\ oop' = NoOop
    /\ finalizedTrace' = finalizedTrace
    /\ postOopRejects' = 0
    /\ frontier' = 0

Next ==
    (\E e \in Events : ReserveOk(e) \/ ReserveOop(e) \/ RejectPostOop(e) \/ RejectInvalid(e)) \/
    FinalizeTrace \/
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

FinalizedTraceSequence ==
    finalizedTrace \in Seq(Events)

FinalizationPreservesActiveBudget ==
    frontier = 1 => consumed <= InitialBudget

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
    frontier = 1 =>
        CanonicalDigestEntries(successTrace, oop) = FinalizedDigestEntries

=============================================================================
