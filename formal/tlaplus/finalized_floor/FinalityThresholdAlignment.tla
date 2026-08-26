----------------------- MODULE FinalityThresholdAlignment -----------------------
EXTENDS Integers

CONSTANTS
  \* @type: Int;
  TotalStake,
  \* @type: Int;
  AgreeingStake,
  \* @type: Int;
  CliqueStake,
  \* @type: Int;
  ThresholdNumerator,
  \* @type: Int;
  ThresholdDenominator,
  \* @type: Bool;
  InclusiveCandidate

ASSUME /\ TotalStake > 0
       /\ AgreeingStake \in 0..TotalStake
       /\ CliqueStake \in 0..AgreeingStake
       /\ ThresholdDenominator > 0
       /\ ThresholdNumerator \in (-ThresholdDenominator)..ThresholdDenominator
       /\ InclusiveCandidate \in BOOLEAN

VARIABLES
  \* @type: Str;
  phase

\* @type: <<Str>>;
vars == <<phase>>

Init == phase = "Check"

Next == phase' = phase

Spec == Init /\ [][Next]_vars

MajorityGate == 2 * AgreeingStake > TotalStake
StrictCertificate ==
  /\ MajorityGate
  /\ 2 * CliqueStake * ThresholdDenominator
       > TotalStake * (ThresholdDenominator + ThresholdNumerator)
InclusiveCertificate ==
  /\ MajorityGate
  /\ 2 * CliqueStake * ThresholdDenominator
       >= TotalStake * (ThresholdDenominator + ThresholdNumerator)

CandidateCertificate ==
  IF InclusiveCandidate THEN InclusiveCertificate ELSE StrictCertificate

DurableFinalizerCertificate == StrictCertificate

CandidateAndFinalizerAgree ==
  CandidateCertificate = DurableFinalizerCertificate

NoUnmaterializableCandidateFloor ==
  CandidateCertificate => DurableFinalizerCertificate

=============================================================================
