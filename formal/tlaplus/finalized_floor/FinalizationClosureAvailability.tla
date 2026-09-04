----------------- MODULE FinalizationClosureAvailability -----------------
EXTENDS Integers, FiniteSets

CONSTANT
  \* @type: Str;
  Defect

ASSUME Defect \in {"None", "MissingAsEmpty", "OutsideCommitteeFinalizer", "LostWake"}

\* @type: Set(Int);
Dependencies == {1, 2}
\* @type: Set(Int);
Committee == {1, 2}
\* @type: Int;
Outsider == 3
\* @type: Set(Int);
Validators == Committee \cup {Outsider}
\* @type: Set(Str);
Phases == {"Idle", "Held", "Certified", "Rejected"}

VARIABLES
  \* @type: Set(Int);
  available,
  \* @type: Set(Int);
  requested,
  \* @type: Int;
  heldFor,
  \* @type: Set(Int);
  projection,
  \* @type: Int;
  floor,
  \* @type: Bool;
  certificate,
  \* @type: Str;
  phase,
  \* @type: Bool;
  invalidClosure,
  \* @type: Bool;
  outsiderPresent
  ,
  \* @type: Bool;
  wakePending

vars == <<available, requested, heldFor, projection, floor, certificate,
          phase, invalidClosure, outsiderPresent, wakePending>>

Missing == Dependencies \ available

Init ==
  /\ available = {}
  /\ requested = {}
  /\ heldFor = 0
  /\ projection = {}
  /\ floor = 0
  /\ certificate = FALSE
  /\ phase = "Idle"
  /\ invalidClosure = FALSE
  /\ outsiderPresent = FALSE
  /\ wakePending = FALSE

ToggleOutsider ==
  /\ outsiderPresent' = ~outsiderPresent
  /\ UNCHANGED <<available, requested, heldFor, projection, floor,
                  certificate, phase, invalidClosure, wakePending>>

RestoreDependency(dependency) ==
  /\ dependency \in Dependencies \ available
  /\ available' = available \cup {dependency}
  /\ requested' = requested \ {dependency}
  /\ wakePending' =
       IF phase = "Held" /\ heldFor = dependency
       THEN Defect # "LostWake"
       ELSE wakePending
  /\ UNCHANGED <<heldFor, projection, floor, certificate, phase,
                  invalidClosure, outsiderPresent>>

DiscoverInvalid ==
  /\ phase \in {"Idle", "Held"}
  /\ invalidClosure' = TRUE
  /\ UNCHANGED <<available, requested, heldFor, projection, floor,
                  certificate, phase, outsiderPresent, wakePending>>

AttemptMissing ==
  /\ phase \in {"Idle", "Held"}
  /\ ~invalidClosure
  /\ Missing # {}
  /\ LET dependency == CHOOSE candidate \in Missing : TRUE IN
       IF Defect = "MissingAsEmpty"
       THEN
         /\ phase' = "Certified"
         /\ floor' = 1
         /\ certificate' = TRUE
         /\ projection' = {}
         /\ heldFor' = 0
         /\ wakePending' = FALSE
         /\ UNCHANGED requested
       ELSE
         /\ phase' = "Held"
         /\ floor' = floor
         /\ certificate' = FALSE
         /\ projection' = {}
         /\ heldFor' = dependency
         /\ requested' = requested \cup {dependency}
         /\ wakePending' = FALSE
  /\ UNCHANGED <<available, invalidClosure, outsiderPresent>>

AttemptComplete ==
  /\ phase \in {"Idle", "Held"}
  /\ ~invalidClosure
  /\ Missing = {}
  /\ phase' = "Certified"
  /\ floor' = 1
  /\ certificate' = TRUE
  /\ projection' =
       IF Defect = "OutsideCommitteeFinalizer" /\ outsiderPresent
       THEN Committee \cup {Outsider}
       ELSE Committee
  /\ heldFor' = 0
  /\ requested' = {}
  /\ wakePending' = FALSE
  /\ UNCHANGED <<available, invalidClosure, outsiderPresent>>

AttemptInvalid ==
  /\ phase \in {"Idle", "Held"}
  /\ invalidClosure
  /\ phase' = "Rejected"
  /\ floor' = 0
  /\ certificate' = FALSE
  /\ projection' = {}
  /\ heldFor' = 0
  /\ requested' = {}
  /\ wakePending' = FALSE
  /\ UNCHANGED <<available, invalidClosure, outsiderPresent>>

RetryRecovered ==
  /\ phase = "Held"
  /\ heldFor \in available
  /\ wakePending
  /\ phase' = "Idle"
  /\ heldFor' = 0
  /\ wakePending' = FALSE
  /\ UNCHANGED <<available, requested, projection, floor, certificate,
                  invalidClosure, outsiderPresent>>

Next ==
  \/ ToggleOutsider
  \/ \E dependency \in Dependencies : RestoreDependency(dependency)
  \/ DiscoverInvalid
  \/ AttemptMissing
  \/ AttemptComplete
  \/ AttemptInvalid
  \/ RetryRecovered

Spec == Init /\ [][Next]_vars

TypeOK ==
  /\ available \subseteq Dependencies
  /\ requested \subseteq Dependencies
  /\ heldFor \in 0..2
  /\ projection \subseteq Validators
  /\ floor \in 0..1
  /\ certificate \in BOOLEAN
  /\ phase \in Phases
  /\ invalidClosure \in BOOLEAN
  /\ outsiderPresent \in BOOLEAN
  /\ wakePending \in BOOLEAN

MissingClosureHasNoAdvance == Missing # {} => floor = 0

MissingClosureHasNoCertificate == Missing # {} => ~certificate

MissingIdentityIsExact ==
  phase = "Held" =>
    /\ heldFor \in Dependencies
    /\ heldFor \in requested \cup available

InvalidClosureCannotCertify == invalidClosure => ~certificate

RecoveredHoldHasWake ==
  phase = "Held" /\ heldFor \in available => wakePending

CertificateRequiresCompleteProjection ==
  certificate =>
    /\ Missing = {}
    /\ ~invalidClosure
    /\ floor = 1
    /\ projection # {}

ProjectionUsesFrozenCommittee == projection \subseteq Committee

OutsiderDoesNotChangeSafeProjection ==
  Defect = "None" /\ certificate => projection = Committee

RestoreSet(current, dependency) == current \cup {dependency}

IndependentRestoresCommute ==
  \A left, right \in Dependencies :
    RestoreSet(RestoreSet(available, left), right) =
      RestoreSet(RestoreSet(available, right), left)

Safety ==
  /\ TypeOK
  /\ MissingClosureHasNoAdvance
  /\ MissingClosureHasNoCertificate
  /\ MissingIdentityIsExact
  /\ InvalidClosureCannotCertify
  /\ RecoveredHoldHasWake
  /\ CertificateRequiresCompleteProjection
  /\ ProjectionUsesFrozenCommittee
  /\ OutsiderDoesNotChangeSafeProjection
  /\ IndependentRestoresCommute

=============================================================================
