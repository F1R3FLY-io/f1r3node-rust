------------------ MODULE ParBagProtobufSinkPublication ------------------
EXTENDS Naturals, TLC

(***************************************************************************
Fallible borrowed-chunk sink lifecycle. Validation failure emits no private
prefix. A sink failure may leave a private prefix, but the owner retains the
old public value until a complete staged image is committed atomically.
***************************************************************************)

CONSTANT MaxBytes

ASSUME MaxBytes \in Nat

Phases == {"preflight", "emitting", "failed", "done", "committed", "rejected"}
PublicImages == {"old", "new", "partial"}

VARIABLES inputValid, target, staging, phase, public

vars == <<inputValid, target, staging, phase, public>>

Init ==
    /\ inputValid \in BOOLEAN
    /\ target \in 0..MaxBytes
    /\ staging = 0
    /\ phase = "preflight"
    /\ public = "old"

AcceptPreflight ==
    /\ phase = "preflight"
    /\ inputValid
    /\ phase' = "emitting"
    /\ UNCHANGED <<inputValid, target, staging, public>>

RejectPreflight ==
    /\ phase = "preflight"
    /\ ~inputValid
    /\ phase' = "rejected"
    /\ UNCHANGED <<inputValid, target, staging, public>>

EmitPrivateByte ==
    /\ phase = "emitting"
    /\ staging < target
    /\ staging' = staging + 1
    /\ UNCHANGED <<inputValid, target, phase, public>>

FailSink ==
    /\ phase = "emitting"
    /\ phase' = "failed"
    /\ UNCHANGED <<inputValid, target, staging, public>>

FinishEmission ==
    /\ phase = "emitting"
    /\ staging = target
    /\ phase' = "done"
    /\ UNCHANGED <<inputValid, target, staging, public>>

CommitComplete ==
    /\ phase = "done"
    /\ public' = "new"
    /\ phase' = "committed"
    /\ UNCHANGED <<inputValid, target, staging>>

Next == AcceptPreflight \/ RejectPreflight \/ EmitPrivateByte
        \/ FailSink \/ FinishEmission \/ CommitComplete

Spec == Init /\ [][Next]_vars

TypeInvariant ==
    /\ inputValid \in BOOLEAN
    /\ target \in 0..MaxBytes
    /\ staging \in 0..target
    /\ phase \in Phases
    /\ public \in PublicImages

InvalidInputHasNoPrefix == ~inputValid => staging = 0

PublicImageAtomic ==
    /\ (public = "new") <=> (phase = "committed")
    /\ public # "partial"

FailedSinkIsNotPublished == phase = "failed" => public = "old"

CommitRequiresCompleteImage == phase = "committed" => staging = target

(***************************************************************************
Negative controls for premature public visibility and emission before the
mandatory complete preflight.
***************************************************************************)
ExposePrivatePrefixMutant ==
    /\ phase = "emitting"
    /\ staging < target
    /\ staging' = staging + 1
    /\ public' = "partial"
    /\ UNCHANGED <<inputValid, target, phase>>

EmitBeforePreflightMutant ==
    /\ phase = "preflight"
    /\ ~inputValid
    /\ staging < target
    /\ staging' = staging + 1
    /\ UNCHANGED <<inputValid, target, phase, public>>

PublicMutantNext == Next \/ ExposePrivatePrefixMutant
PublicMutantSpec == Init /\ [][PublicMutantNext]_vars

PreflightMutantNext == Next \/ EmitBeforePreflightMutant
PreflightMutantSpec == Init /\ [][PreflightMutantNext]_vars

=============================================================================
