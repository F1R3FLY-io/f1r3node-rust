(* ::Package:: *)

(* reservation_admission_regions.wl - Correctness-constrained exploration of
   cost-accounting reservation, refund, and admission-capacity policies.

   The authoritative conservation and concurrency proofs remain in Rocq,
   TLA+/Apalache, Loom, and Rust. This model asks the distinct pre-benchmark
   question: after component-wise located-purse safety is mandatory, which
   reservation policy is minimally restrictive, how much capital can path
   correlation save, and how much admission concurrency can purse locality
   expose before implementation profiling supplies machine constants?
*)

ClearAll["Global`*"];

paths = 3;
purses = 2;
demandLimit = 3;
supplyLimit = paths demandLimit;

demandMatrices = Partition[#, purses] & /@
  Tuples[Range[0, demandLimit], paths purses];
supplies = Tuples[Range[0, supplyLimit], purses];

perPurseMaximum[matrix_] := Max /@ Transpose[matrix];
perPursePathSum[matrix_] := Total /@ Transpose[matrix];
pooledMaximum[matrix_] := Max[Total /@ matrix];

componentwiseSafeQ[supply_, matrix_] :=
  And @@ (And @@ Thread[supply >= #] & /@ matrix);

policies = {
  "FirstBranch",
  "PooledScalar",
  "PerPursePathSum",
  "PerPurseMaximum"
};

acceptsQ["FirstBranch", supply_, matrix_] :=
  And @@ Thread[supply >= First[matrix]];
acceptsQ["PooledScalar", supply_, matrix_] :=
  Total[supply] >= pooledMaximum[matrix];
acceptsQ["PerPursePathSum", supply_, matrix_] :=
  And @@ Thread[supply >= perPursePathSum[matrix]];
acceptsQ["PerPurseMaximum", supply_, matrix_] :=
  And @@ Thread[supply >= perPurseMaximum[matrix]];

policyFailures[policy_] := <|
  "UnsafeAcceptance" -> Count[
    Flatten[Table[
      acceptsQ[policy, supply, matrix] &&
        ! componentwiseSafeQ[supply, matrix],
      {matrix, demandMatrices}, {supply, supplies}]],
    True],
  "SafeRejection" -> Count[
    Flatten[Table[
      ! acceptsQ[policy, supply, matrix] &&
        componentwiseSafeQ[supply, matrix],
      {matrix, demandMatrices}, {supply, supplies}]],
    True]
|>;

policyResults = AssociationMap[policyFailures, policies];
feasibleExactPolicies = Select[policies,
  Total[Values[policyResults[#]]] == 0 &];

candidateReservations = Tuples[Range[0, demandLimit], purses];
minimalityFailures = Count[
  Flatten[Table[
    ! componentwiseSafeQ[reserve, matrix] ||
      And @@ Thread[reserve >= perPurseMaximum[matrix]],
    {matrix, demandMatrices}, {reserve, candidateReservations}]],
  False];

locatedEnvelope[matrix_] := Total[perPurseMaximum[matrix]];
pooledEnvelope[matrix_] := pooledMaximum[matrix];
locatedGaps = locatedEnvelope[#] - pooledEnvelope[#] & /@ demandMatrices;
locatedGapDistribution = Counts[locatedGaps];

oneSurfaceMatrices = Flatten[
  Table[{{a, fixed}, {b, fixed}, {c, fixed}},
    {a, 0, demandLimit}, {b, 0, demandLimit},
    {c, 0, demandLimit}, {fixed, 0, demandLimit}],
  3];
oneSurfaceGapFailures = Count[
  locatedEnvelope[#] - pooledEnvelope[#] & /@ oneSurfaceMatrices,
  Except[0]];

refundFailures = Count[
  Flatten[Table[
    With[{reserve = perPurseMaximum[matrix], demand = matrix[[path]]},
      And @@ Thread[reserve - demand >= 0] &&
        demand + (reserve - demand) === reserve],
    {matrix, demandMatrices}, {path, 1, paths}]],
  False];

resourceDimensions = 5;
resourceMatrices = Partition[#, resourceDimensions] & /@
  Tuples[{0, 1}, paths resourceDimensions];
priceVectors = Tuples[{1, 2}, resourceDimensions];

pricedPathEnvelope[matrix_, prices_] :=
  Max[Dot[prices, #] & /@ matrix];
pricedComponentEnvelope[matrix_, prices_] :=
  Dot[prices, Max /@ Transpose[matrix]];

resourceEnvelopeGaps = Flatten[Table[
  pricedComponentEnvelope[matrix, prices] -
    pricedPathEnvelope[matrix, prices],
  {matrix, resourceMatrices}, {prices, priceVectors}]];
resourceEnvelopeFailures = Count[resourceEnvelopeGaps, _?(# < 0 &)];
resourceEnvelopeStrictWitnesses = Count[resourceEnvelopeGaps, _?(# > 0 &)];

deployments = 4;
capacityPurses = 3;
unitCapacity = ConstantArray[1, capacityPurses];
nonemptyRequirements = DeleteCases[
  Tuples[{0, 1}, capacityPurses],
  ConstantArray[0, capacityPurses]];
workloadMatrices = Tuples[nonemptyRequirements, deployments];
deploymentSubsets = Subsets[Range[deployments]];

feasibleSubsetQ[requirements_, subset_] := subset === {} ||
  And @@ Thread[Total[requirements[[subset]]] <= unitCapacity];
maxLocatedConcurrency[requirements_] := Max[
  Length /@ Select[deploymentSubsets,
    feasibleSubsetQ[requirements, #] &]];

locatedConcurrency = maxLocatedConcurrency /@ workloadMatrices;
locatedConcurrencyDistribution = Counts[locatedConcurrency];
singletonLedgerConcurrency = 1;
concurrencyImprovementCases = Count[
  locatedConcurrency,
  _?(# > singletonLedgerConcurrency &)];
disjointWitness = maxLocatedConcurrency[{
  {1, 0, 0}, {0, 1, 0}, {0, 0, 1}, {1, 0, 0}
}] == 3;

ClearAll[reserveOne, reserveTwo];
solverDemand = {{3, 0}, {0, 2}, {2, 1}};
minimumReserveSolution = Minimize[{
    reserveOne + reserveTwo,
    reserveOne >= 0 && reserveTwo >= 0 &&
      And @@ (And @@ Thread[{reserveOne, reserveTwo} >= #] & /@
        solverDemand) &&
      Element[{reserveOne, reserveTwo}, Integers]
  },
  {reserveOne, reserveTwo}];
minimumReserveMatchesEnvelope =
  First[minimumReserveSolution] == Total[perPurseMaximum[solverDemand]] &&
    ({reserveOne, reserveTwo} /. Last[minimumReserveSolution]) ===
      perPurseMaximum[solverDemand];

selectionVariables = Array[selected, deployments];
solverWorkload = {
  {1, 0, 0}, {0, 1, 0}, {0, 0, 1}, {1, 0, 0}
};
maximumAdmissionSolution = Maximize[{
    Total[selectionVariables],
    And @@ Thread[
      Transpose[solverWorkload].selectionVariables <= unitCapacity] &&
      And @@ Thread[0 <= selectionVariables <= 1] &&
      Element[selectionVariables, Integers]
  },
  selectionVariables];
maximumAdmissionMatchesEnumeration =
  First[maximumAdmissionSolution] ==
    maxLocatedConcurrency[solverWorkload];

ClearAll[branchCount, surfaceCount];
pathEnumerationWork = 2^branchCount surfaceCount;
compositionalWork = branchCount surfaceCount;
workRatioGrowth = FullSimplify[
  (2^(branchCount + 1)/(branchCount + 1))/
    (2^branchCount/branchCount),
  branchCount > 0];
workRatioStrictlyIncreasing = FullSimplify[
  workRatioGrowth > 1,
  branchCount > 1];
workRatioUnboundedBase = 2^4/4 >= 4;
workRatioUnboundedStep = FullSimplify[
  Implies[
    2^branchCount/branchCount >= branchCount,
    2^(branchCount + 1)/(branchCount + 1) >= branchCount + 1],
  Element[branchCount, Integers] && branchCount >= 4];

Print["[reservation_admission_regions] exact demand matrices: ",
  Length[demandMatrices]];
Print["[reservation_admission_regions] supplies per matrix: ",
  Length[supplies]];
Print["[reservation_admission_regions] policy failures: ", policyResults];
Print["[reservation_admission_regions] feasible exact policies: ",
  feasibleExactPolicies];
Print["[reservation_admission_regions] component-minimality failures: ",
  minimalityFailures];
Print["[reservation_admission_regions] located-minus-pooled gap distribution: ",
  locatedGapDistribution];
Print["[reservation_admission_regions] one-variable-surface gap failures: ",
  oneSurfaceGapFailures];
Print["[reservation_admission_regions] refund/conservation failures: ",
  refundFailures];
Print["[reservation_admission_regions] priced resource-envelope cases: ",
  Length[resourceEnvelopeGaps]];
Print["[reservation_admission_regions] resource-envelope underbounds: ",
  resourceEnvelopeFailures];
Print["[reservation_admission_regions] strict resource-envelope gaps: ",
  resourceEnvelopeStrictWitnesses];
Print["[reservation_admission_regions] located concurrency distribution: ",
  locatedConcurrencyDistribution];
Print["[reservation_admission_regions] locality improvement cases: ",
  concurrencyImprovementCases];
Print["[reservation_admission_regions] disjoint-purse concurrency witness: ",
  disjointWitness];
Print["[reservation_admission_regions] exact minimum-reserve solution: ",
  minimumReserveSolution];
Print["[reservation_admission_regions] reserve optimizer matches envelope: ",
  minimumReserveMatchesEnvelope];
Print["[reservation_admission_regions] exact maximum-admission solution: ",
  maximumAdmissionSolution];
Print["[reservation_admission_regions] admission optimizer matches enumeration: ",
  maximumAdmissionMatchesEnumeration];
Print["[reservation_admission_regions] path/compositional growth factor: ",
  workRatioGrowth];
Print["[reservation_admission_regions] work ratio strictly increases: ",
  workRatioStrictlyIncreasing];
Print["[reservation_admission_regions] unbounded work-ratio induction: ",
  {workRatioUnboundedBase, workRatioUnboundedStep}];

pass = Length[demandMatrices] == 4096 && Length[supplies] == 100 &&
  feasibleExactPolicies === {"PerPurseMaximum"} &&
  minimalityFailures == 0 && Min[locatedGaps] == 0 &&
  Max[locatedGaps] > 0 && oneSurfaceGapFailures == 0 &&
  refundFailures == 0 && resourceEnvelopeFailures == 0 &&
  resourceEnvelopeStrictWitnesses > 0 &&
  Min[locatedConcurrency] == 1 && Max[locatedConcurrency] == 3 &&
  concurrencyImprovementCases > 0 && TrueQ[disjointWitness] &&
  TrueQ[minimumReserveMatchesEnvelope] &&
  TrueQ[maximumAdmissionMatchesEnumeration] &&
  workRatioGrowth === 2 branchCount/(1 + branchCount) &&
  TrueQ[workRatioStrictlyIncreasing] && TrueQ[workRatioUnboundedBase] &&
  TrueQ[workRatioUnboundedStep];
Print["[reservation_admission_regions] SELF-TEST: ",
  If[pass, "PASS", "FAIL"]];
If[! pass, Exit[1]];
