------------------ MODULE ParBagProtobufSinkPublication ------------------
EXTENDS Naturals, TLC

(***************************************************************************
Fallible private-frame lifecycle. Validation failure emits no prefix. A sink
failure may have produced a temporary private prefix, but the whole private
frame becomes inaccessible and the owner retains its old public value.
***************************************************************************)

CONSTANT MaxBytes

ASSUME MaxBytes \in Nat

Phases == {"preflight", "emitting", "failed", "done", "committed", "rejected"}
PublicImages == {"old", "new", "partial"}

VARIABLES inputValid, target, staging, phase, public, privateAccessible

vars == <<inputValid, target, staging, phase, public, privateAccessible>>

Init ==
    /\ inputValid \in BOOLEAN
    /\ target \in 0..MaxBytes
    /\ staging = 0
    /\ phase = "preflight"
    /\ public = "old"
    /\ privateAccessible = TRUE

AcceptPreflight ==
    /\ phase = "preflight"
    /\ inputValid
    /\ phase' = "emitting"
    /\ UNCHANGED <<inputValid, target, staging, public, privateAccessible>>

RejectPreflight ==
    /\ phase = "preflight"
    /\ ~inputValid
    /\ phase' = "rejected"
    /\ privateAccessible' = FALSE
    /\ UNCHANGED <<inputValid, target, staging, public>>

EmitPrivateByte ==
    /\ phase = "emitting"
    /\ staging < target
    /\ staging' = staging + 1
    /\ UNCHANGED <<inputValid, target, phase, public, privateAccessible>>

FailSink ==
    /\ phase = "emitting"
    /\ phase' = "failed"
    /\ privateAccessible' = FALSE
    /\ UNCHANGED <<inputValid, target, staging, public>>

FinishEmission ==
    /\ phase = "emitting"
    /\ staging = target
    /\ phase' = "done"
    /\ UNCHANGED <<inputValid, target, staging, public, privateAccessible>>

CommitComplete ==
    /\ phase = "done"
    /\ public' = "new"
    /\ phase' = "committed"
    /\ UNCHANGED <<inputValid, target, staging, privateAccessible>>

Next == AcceptPreflight \/ RejectPreflight \/ EmitPrivateByte
        \/ FailSink \/ FinishEmission \/ CommitComplete

Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ inputValid \in BOOLEAN
    /\ target \in 0..MaxBytes
    /\ staging \in 0..target
    /\ phase \in Phases
    /\ public \in PublicImages
    /\ privateAccessible \in BOOLEAN

InvalidInputHasNoPrefix == ~inputValid => staging = 0

PublicImageAtomic ==
    /\ (public = "new") <=> (phase = "committed")
    /\ public # "partial"

FailedSinkIsNotPublished == phase = "failed" => public = "old"

CommitRequiresCompleteImage == phase = "committed" => staging = target

AbortedPrivateFrameIsDiscarded ==
    phase \in {"failed", "rejected"} => ~privateAccessible

(***************************************************************************
Negative controls for premature public visibility and emission before the
mandatory complete preflight.
***************************************************************************)
ExposePrivatePrefixMutant ==
    /\ phase = "emitting"
    /\ staging < target
    /\ staging' = staging + 1
    /\ public' = "partial"
    /\ UNCHANGED <<inputValid, target, phase, privateAccessible>>

EmitBeforePreflightMutant ==
    /\ phase = "preflight"
    /\ ~inputValid
    /\ staging < target
    /\ staging' = staging + 1
    /\ UNCHANGED <<inputValid, target, phase, public, privateAccessible>>

PrematureCommitMutant ==
    /\ phase = "emitting"
    /\ staging < target
    /\ phase' = "committed"
    /\ public' = "new"
    /\ UNCHANGED <<inputValid, target, staging, privateAccessible>>

RollbackMutant ==
    /\ phase = "emitting"
    /\ staging > 0
    /\ phase' = "failed"
    /\ UNCHANGED <<inputValid, target, staging, public, privateAccessible>>

PublicMutantNext == Next \/ ExposePrivatePrefixMutant
PublicMutantSpec == Init /\ [][PublicMutantNext]_vars

PreflightMutantNext == Next \/ EmitBeforePreflightMutant
PreflightMutantSpec == Init /\ [][PreflightMutantNext]_vars

PrematureCommitMutantNext == Next \/ PrematureCommitMutant
PrematureCommitMutantSpec == Init /\ [][PrematureCommitMutantNext]_vars

RollbackMutantNext == Next \/ RollbackMutant
RollbackMutantSpec == Init /\ [][RollbackMutantNext]_vars

=============================================================================
