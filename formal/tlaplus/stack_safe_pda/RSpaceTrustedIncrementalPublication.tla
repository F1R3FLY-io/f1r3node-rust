---------------- MODULE RSpaceTrustedIncrementalPublication ----------------
EXTENDS Naturals, TLC

(***************************************************************************
Trusted incremental publication of a durable RSpace successor (J-067).

An incremental publication appends one sealed transaction to the active pack,
swaps the catalog slot and returns the successor authority.  The cold-checked
mode returns the successor only after physical recovery and cold admission of
the published catalog under the caller's incoming admission limits.  The
trusted mode returns the successor core derived before any write: the bind
runs the ledger trials in domain order (Current, Retained, Metadata) and
checks the final ceilings of a hot policy, and the sealed transaction carries
the mode and the policy.  After the swap the trusted mode performs only the
catalog-directory sync, the reclamation-sidecar resolution and an exact
re-read of the catalog image.

The model abstracts every count, ledger and byte image to the order of the
decisions, the mutations and the evidence each mode returns.  That the core
the bind derives equals the cold admission of the successor image is proved
in TrustedSuccessorCapability.v; here the core is adopted only while the
catalog holds exactly the image the swap wrote.

A run fixes a scenario at Init:

* the mode the transaction is sealed with and the mode of the publication
  call;
* the margin of the sealed limits against the measured requirement: exact,
  which every trial and ceiling accepts, or one below on some axis, which
  refuses;
* whether the publication call passes the sealed policy or a changed one;
* whether the successor index assembles or meets a record conflict; and
* the incoming cold admission limits: loose, which admit the successor, or
  tight, which refuse it.

The environment may replace the catalog image with one this publication did
not write (a foreign, stale or resequenced authority, or external damage)
before the preflight, between the preflight and the swap, and between the
catalog file sync and adoption.  The swap's own verify-then-write window is
excluded: the exclusive writer lease owns it.  Damage after adoption is the
trade J-067 accepts and is found at the next cold boundary.

Nine invariants hold in the base configuration.  Eight negative controls each
break one design decision, and six non-vacuity witnesses show that trusted
success under tight cold limits, cold success, a trusted policy refusal, a
published resolution failure, a stage failure before the swap and an
indeterminate catalog write are all reachable.
***************************************************************************)

Modes == {"trusted", "cold"}
Margins == {"exact", "minus-one"}
CallPolicies == {"sealed", "changed"}
Assemblies == {"ok", "conflict"}
ColdLimits == {"loose", "tight"}

(* "old" is the predecessor image the core's token names, "new" the successor
   image the swap writes, "torn" a partial slot write, and "other" an image
   this publication did not write. *)
Images == {"old", "new", "torn", "other"}

Trials == {"pending", "accepted", "refused"}
Evidence == {"none", "measured", "fabricated"}
Refusals ==
    {"none", "trusted-policy", "cold-limits", "mode-mismatch",
     "policy-mismatch", "stale-authority", "assembly"}
Failures ==
    {"none", "io", "stage", "cas-mismatch", "slot-write", "directory",
     "sidecar-resolution", "reread-mismatch", "cold-recovery", "cold-admission"}
Outcomes ==
    {"none", "definitely-not-published", "indeterminate", "published-error",
     "ready"}
Phases ==
    {"Idle", "Bound", "Preflighted", "IndexAssembled", "Appended", "Staged",
     "CasVerified", "SlotWriting", "CatalogFileDurable", "DirectoryDurable",
     "Resolved", "ColdRecovered", "Ready", "Refused", "Failed"}
Terminal == {"Ready", "Refused", "Failed"}

VARIABLE run

vars == <<run>>

Init ==
    \E sealedMode \in Modes, callMode \in Modes, margin \in Margins,
       callPolicy \in CallPolicies, assembly \in Assemblies,
       coldLimit \in ColdLimits :
      run = [sealedMode |-> sealedMode, callMode |-> callMode,
             margin |-> margin, callPolicy |-> callPolicy,
             assembly |-> assembly, coldLimit |-> coldLimit,
             phase |-> "Idle", trial |-> "pending", checked |-> FALSE,
             mutated |-> FALSE, slotWriteStarted |-> FALSE,
             catalog |-> "old", catalogFileDurable |-> FALSE,
             dirSynced |-> FALSE, resolved |-> FALSE, recovered |-> FALSE,
             coldAdmitted |-> FALSE, evidence |-> "none",
             adopted |-> "none", refusal |-> "none", failure |-> "none",
             outcome |-> "none"]

Refuse(kind) ==
    run' = [run EXCEPT !.phase = "Refused", !.refusal = kind,
                       !.outcome = "definitely-not-published"]

Fail(kind, outcome) ==
    run' = [run EXCEPT !.phase = "Failed", !.failure = kind,
                       !.outcome = outcome]

(***************************************************************************
The bind runs the ledger trials and checks the final ceilings against the
sealed limits: the hot policy in the trusted mode, the limits the cold-checked
bind derives in the cold mode.  It writes nothing.
***************************************************************************)
Bind ==
    /\ run.phase = "Idle"
    /\ IF run.margin = "exact"
       THEN run' = [run EXCEPT !.phase = "Bound", !.trial = "accepted"]
       ELSE run' = [run EXCEPT !.phase = "Refused", !.trial = "refused",
                      !.refusal = IF run.sealedMode = "trusted"
                                  THEN "trusted-policy" ELSE "cold-limits",
                      !.outcome = "definitely-not-published"]

(* The publication preflight: the call's mode and, in the trusted mode, its
   policy must equal the sealed ones, and the catalog must still hold the
   predecessor image the core's token names. *)
PolicyPreflight ==
    /\ run.phase = "Bound"
    /\ CASE run.callMode # run.sealedMode -> Refuse("mode-mismatch")
         [] run.callMode = "trusted" /\ run.callPolicy = "changed" ->
              Refuse("policy-mismatch")
         [] run.catalog # "old" -> Refuse("stale-authority")
         [] OTHER -> run' = [run EXCEPT !.phase = "Preflighted",
                                        !.checked = TRUE]

(* The trusted mode builds the successor object index in memory before the
   append, so a duplicate or conflicting record refuses before mutation. *)
AssembleSuccessorIndex ==
    /\ run.phase = "Preflighted"
    /\ run.callMode = "trusted"
    /\ IF run.assembly = "ok"
       THEN run' = [run EXCEPT !.phase = "IndexAssembled"]
       ELSE Refuse("assembly")

(* The truncation, PREPARE, index and commit writes, their syncs and the
   artifact-directory sync: the same code and bytes in both modes. *)
AppendCommit ==
    /\ \/ run.phase = "IndexAssembled" /\ run.callMode = "trusted"
       \/ run.phase = "Preflighted" /\ run.callMode = "cold"
    /\ run' = [run EXCEPT !.phase = "Appended", !.mutated = TRUE]

(* The trusted mode derives the successor catalog token and pins the store
   before the swap. *)
Stage ==
    /\ run.phase = "Appended"
    /\ run.callMode = "trusted"
    /\ run' = [run EXCEPT !.phase = "Staged"]

CasVerify ==
    /\ \/ run.phase = "Staged" /\ run.callMode = "trusted"
       \/ run.phase = "Appended" /\ run.callMode = "cold"
    /\ IF run.catalog = "old"
       THEN run' = [run EXCEPT !.phase = "CasVerified"]
       ELSE Fail("cas-mismatch", "definitely-not-published")

SlotWrite ==
    /\ run.phase = "CasVerified"
    /\ run' = [run EXCEPT !.phase = "SlotWriting", !.slotWriteStarted = TRUE]

CompleteCatalogWrite ==
    /\ run.phase = "SlotWriting"
    /\ run' = [run EXCEPT !.phase = "CatalogFileDurable", !.catalog = "new",
                          !.catalogFileDurable = TRUE]

Cas == CasVerify \/ SlotWrite \/ CompleteCatalogWrite

DirSync ==
    /\ run.phase = "CatalogFileDurable"
    /\ run' = [run EXCEPT !.phase = "DirectoryDurable", !.dirSynced = TRUE]

ResolveSidecar ==
    /\ run.phase = "DirectoryDurable"
    /\ run.callMode = "trusted"
    /\ run' = [run EXCEPT !.phase = "Resolved", !.resolved = TRUE]

(* Adoption re-reads the catalog and compares it exactly with the image the
   swap wrote.  The returned core carries no cold evidence. *)
AdoptTrusted ==
    /\ run.phase = "Resolved"
    /\ run.callMode = "trusted"
    /\ IF run.catalog = "new"
       THEN run' = [run EXCEPT !.phase = "Ready", !.adopted = "new",
                               !.outcome = "ready"]
       ELSE Fail("reread-mismatch", "published-error")

(* Cold-checked mode: physical recovery, which also resolves the sidecar,
   then cold admission under the incoming limits. *)
ColdRecover ==
    /\ run.phase = "DirectoryDurable"
    /\ run.callMode = "cold"
    /\ IF run.catalog = "new"
       THEN run' = [run EXCEPT !.phase = "ColdRecovered", !.recovered = TRUE,
                               !.resolved = TRUE]
       ELSE Fail("cold-recovery", "published-error")

ColdAdmit ==
    /\ run.phase = "ColdRecovered"
    /\ IF run.coldLimit = "loose"
       THEN run' = [run EXCEPT !.phase = "Ready", !.coldAdmitted = TRUE,
                               !.evidence = "measured",
                               !.adopted = run.catalog, !.outcome = "ready"]
       ELSE Fail("cold-admission", "published-error")

ColdRecoverAndAdmit == ColdRecover \/ ColdAdmit

(***************************************************************************
Failures and their outcome classes.
***************************************************************************)
FailBeforeCas ==
    /\ run.phase \in {"Preflighted", "IndexAssembled", "Appended", "Staged",
                      "CasVerified"}
    /\ Fail("io", "definitely-not-published")

FailStage ==
    /\ run.phase = "Appended"
    /\ run.callMode = "trusted"
    /\ Fail("stage", "definitely-not-published")

FailCatalogWrite(persisted) ==
    /\ run.phase = "SlotWriting"
    /\ run' = [run EXCEPT !.phase = "Failed", !.catalog = persisted,
                          !.failure = "slot-write",
                          !.outcome = "indeterminate"]

FailDirSync ==
    /\ run.phase = "CatalogFileDurable"
    /\ Fail("directory", "published-error")

FailResolution ==
    /\ run.phase = "DirectoryDurable"
    /\ run.callMode = "trusted"
    /\ Fail("sidecar-resolution", "published-error")

(* An I/O failure at the hooks that run after the catalog-directory sync and
   before the sidecar resolution or the physical recovery, in either mode. *)
FailAfterDirectorySync ==
    /\ run.phase = "DirectoryDurable"
    /\ Fail("io", "published-error")

ExternalCatalogChange ==
    /\ run.phase \in {"Idle", "Bound", "Preflighted", "IndexAssembled",
                      "Appended", "Staged", "CatalogFileDurable",
                      "DirectoryDurable", "Resolved"}
    /\ run.catalog \in {"old", "new"}
    /\ run' = [run EXCEPT !.catalog = "other"]

Next ==
    \/ Bind \/ PolicyPreflight \/ AssembleSuccessorIndex \/ AppendCommit
    \/ Stage \/ Cas \/ DirSync \/ ResolveSidecar \/ AdoptTrusted
    \/ ColdRecoverAndAdmit
    \/ FailBeforeCas \/ FailStage
    \/ \E persisted \in {"old", "torn", "new"} : FailCatalogWrite(persisted)
    \/ FailDirSync \/ FailAfterDirectorySync \/ FailResolution
    \/ ExternalCatalogChange

Spec == Init /\ [][Next]_vars

(***************************************************************************
Invariants of the base configuration.
***************************************************************************)
TrustedPublicationTypeInvariant ==
    run \in [sealedMode : Modes, callMode : Modes, margin : Margins,
             callPolicy : CallPolicies, assembly : Assemblies,
             coldLimit : ColdLimits, phase : Phases, trial : Trials,
             checked : BOOLEAN, mutated : BOOLEAN,
             slotWriteStarted : BOOLEAN, catalog : Images,
             catalogFileDurable : BOOLEAN, dirSynced : BOOLEAN,
             resolved : BOOLEAN, recovered : BOOLEAN,
             coldAdmitted : BOOLEAN, evidence : Evidence,
             adopted : {"none"} \cup Images, refusal : Refusals,
             failure : Failures, outcome : Outcomes]

(* A refusal leaves every file, the catalog and the caller's authority
   untouched, and every mutation follows accepted trials and an exact
   preflight. *)
TrustedRefusalPrecedesAllMutation ==
    /\ run.refusal # "none" =>
         /\ ~run.mutated
         /\ ~run.slotWriteStarted
         /\ run.catalog # "new"
         /\ run.adopted = "none"
         /\ run.outcome = "definitely-not-published"
    /\ run.mutated => run.trial = "accepted" /\ run.checked

PublishedPolicyIsSealedPolicy ==
    (run.mutated \/ run.outcome = "ready") =>
      /\ run.callMode = run.sealedMode
      /\ run.callMode = "trusted" => run.callPolicy = "sealed"

TrustedSuccessorHasNoColdEvidence ==
    run.callMode = "trusted" => run.evidence = "none"

NoPhysicalRecoveryOnTrustedSuccess ==
    (run.callMode = "trusted" /\ run.outcome = "ready") =>
      /\ ~run.recovered
      /\ ~run.coldAdmitted

ColdModeSuccessIncludesColdAdmission ==
    (run.callMode = "cold" /\ run.outcome = "ready") =>
      /\ run.recovered
      /\ run.coldAdmitted
      /\ run.evidence = "measured"
      /\ run.coldLimit = "loose"

AdoptionFollowsDirectorySyncAndResolution ==
    run.adopted # "none" =>
      /\ run.catalogFileDurable
      /\ run.dirSynced
      /\ run.resolved

AdoptedSuccessorIsExactCatalog ==
    run.adopted # "none" =>
      /\ run.adopted = "new"
      /\ run.catalog = "new"

TrustedOutcomeBoundaryIsExact ==
    /\ run.outcome = "definitely-not-published" =>
         /\ ~run.slotWriteStarted
         /\ run.catalog # "new"
         /\ run.adopted = "none"
    /\ run.outcome = "indeterminate" =>
         /\ run.slotWriteStarted
         /\ ~run.catalogFileDurable
         /\ run.adopted = "none"
    /\ run.outcome = "published-error" =>
         /\ run.catalogFileDurable
         /\ run.adopted = "none"
    /\ run.outcome = "ready" <=> run.adopted # "none"
    /\ run.outcome = "none" <=> run.phase \notin Terminal

(***************************************************************************
Non-vacuity witnesses.  Each must be violated by the faithful specification.
NeverTrustedReady requires tight cold limits, so its counterexample is a
trusted success that cold admission of the same catalog would refuse.
***************************************************************************)
NeverTrustedReady ==
    ~(run.callMode = "trusted" /\ run.outcome = "ready" /\
      run.coldLimit = "tight")

NeverColdReady == ~(run.callMode = "cold" /\ run.outcome = "ready")

NeverTrustedRefusal == run.refusal # "trusted-policy"

NeverPublishedResolutionFailure ==
    ~(run.outcome = "published-error" /\ run.failure = "sidecar-resolution")

NeverStageFailureBeforeCas ==
    ~(run.outcome = "definitely-not-published" /\ run.failure = "stage")

NeverIndeterminate == run.outcome # "indeterminate"

(***************************************************************************
Negative control: the bind defers the ceiling decision, so a policy one below
the measured requirement is refused only after the append has mutated the
pack.
***************************************************************************)
DeferredCeilingBind ==
    /\ run.phase = "Idle"
    /\ run.sealedMode = "trusted"
    /\ run.margin = "minus-one"
    /\ run' = [run EXCEPT !.phase = "Bound", !.trial = "accepted"]

LateCeilingRefusal ==
    /\ run.phase = "Appended"
    /\ run.margin = "minus-one"
    /\ Refuse("trusted-policy")

LateTrustedRefusalMutantNext ==
    Next \/ DeferredCeilingBind \/ LateCeilingRefusal
LateTrustedRefusalMutantSpec == Init /\ [][LateTrustedRefusalMutantNext]_vars

(***************************************************************************
Negative control: trusted adoption attaches the predecessor's cold admission
report instead of returning no cold evidence.
***************************************************************************)
AdoptWithFabricatedEvidence ==
    /\ run.phase = "Resolved"
    /\ run.callMode = "trusted"
    /\ run.catalog = "new"
    /\ run' = [run EXCEPT !.phase = "Ready", !.adopted = "new",
                          !.evidence = "fabricated", !.outcome = "ready"]

FabricatedColdEvidenceMutantNext == Next \/ AdoptWithFabricatedEvidence
FabricatedColdEvidenceMutantSpec ==
    Init /\ [][FabricatedColdEvidenceMutantNext]_vars

(***************************************************************************
Negative control: the preflight checks the mode and the authority but not
the policy, so a policy other than the sealed one reaches publication.
***************************************************************************)
PreflightIgnoringPolicy ==
    /\ run.phase = "Bound"
    /\ run.callMode = run.sealedMode
    /\ run.catalog = "old"
    /\ run' = [run EXCEPT !.phase = "Preflighted", !.checked = TRUE]

ChangedPolicyMutantNext == Next \/ PreflightIgnoringPolicy
ChangedPolicyMutantSpec == Init /\ [][ChangedPolicyMutantNext]_vars

(***************************************************************************
Negative control: the append starts before the bind has run the trials, so a
successor whose trials would refuse reaches the pack and the catalog.
***************************************************************************)
AppendBeforeTrials ==
    /\ run.phase = "Idle"
    /\ run.sealedMode = "trusted"
    /\ run.callMode = "trusted"
    /\ run' = [run EXCEPT !.phase = "Appended", !.mutated = TRUE]

PublicationBeforeTrialsMutantNext == Next \/ AppendBeforeTrials
PublicationBeforeTrialsMutantSpec ==
    Init /\ [][PublicationBeforeTrialsMutantNext]_vars

(***************************************************************************
Negative control: the trusted mode runs physical recovery before adoption,
as the cold-checked mode does.
***************************************************************************)
RecoverBeforeTrustedAdoption ==
    /\ run.phase = "Resolved"
    /\ run.callMode = "trusted"
    /\ ~run.recovered
    /\ run' = [run EXCEPT !.recovered = TRUE]

TrustedRecoveryMutantNext == Next \/ RecoverBeforeTrustedAdoption
TrustedRecoveryMutantSpec == Init /\ [][TrustedRecoveryMutantNext]_vars

(***************************************************************************
Negative control: the core is adopted right after the catalog file sync,
before the directory sync and the sidecar resolution.
***************************************************************************)
AdoptBeforeDirectorySync ==
    /\ run.phase = "CatalogFileDurable"
    /\ run.callMode = "trusted"
    /\ run.catalog = "new"
    /\ run' = [run EXCEPT !.phase = "Ready", !.adopted = "new",
                          !.outcome = "ready"]

EarlyAdoptionMutantNext == Next \/ AdoptBeforeDirectorySync
EarlyAdoptionMutantSpec == Init /\ [][EarlyAdoptionMutantNext]_vars

(***************************************************************************
Negative control: adoption skips the catalog re-read, so a replaced catalog
is answered with the staged core.
***************************************************************************)
AdoptWithoutReread ==
    /\ run.phase = "Resolved"
    /\ run.callMode = "trusted"
    /\ run' = [run EXCEPT !.phase = "Ready", !.adopted = "new",
                          !.outcome = "ready"]

WrongCatalogAdoptionMutantNext == Next \/ AdoptWithoutReread
WrongCatalogAdoptionMutantSpec ==
    Init /\ [][WrongCatalogAdoptionMutantNext]_vars

(***************************************************************************
Negative control: the cold-checked mode returns after physical recovery
without cold admission.
***************************************************************************)
ColdReadyWithoutAdmission ==
    /\ run.phase = "ColdRecovered"
    /\ run' = [run EXCEPT !.phase = "Ready", !.adopted = run.catalog,
                          !.outcome = "ready"]

SkippedColdAdmissionMutantNext == Next \/ ColdReadyWithoutAdmission
SkippedColdAdmissionMutantSpec ==
    Init /\ [][SkippedColdAdmissionMutantNext]_vars

=============================================================================
