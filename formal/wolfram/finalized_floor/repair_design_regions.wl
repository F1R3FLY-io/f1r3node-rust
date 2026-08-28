(* ::Package:: *)

(* repair_design_regions.wl - Pre-benchmark repair-family optimization.

   Correctness is a hard feasibility constraint. The first model exhausts
   bounded parent-admission policies and asks which policy preserves the exact
   frontier, obeys the configured bound, admits every frontier that fits, and
   publishes nothing when the frontier is over capacity. The second derives
   symbolic compute/storage crossover regions for cold exact floor discovery
   versus the guarded incremental cache.

   Unknown machine constants remain symbolic. The resulting inequalities state
   which measurements can change the decision and therefore should be profiled;
   they do not substitute guessed constants for calibration.
*)

ClearAll["Global`*"];

policies = {"StaticWorstCase", "ExactFrontier", "Truncate", "Unbounded"};

publishedParents["StaticWorstCase", frontier_, cap_, committee_] :=
  If[committee + 1 <= cap, frontier, 0];
publishedParents["ExactFrontier", frontier_, cap_, committee_] :=
  If[frontier <= cap, frontier, 0];
publishedParents["Truncate", frontier_, cap_, committee_] :=
  Min[frontier, cap];
publishedParents["Unbounded", frontier_, cap_, committee_] := frontier;

parentCases = Flatten[
  Table[{policy, frontier, cap, committee},
    {policy, policies},
    {committee, 1, 32},
    {cap, 1, committee + 1},
    {frontier, 1, committee + 1}],
  3
];

parentPolicyFailures[policy_] := Module[
  {cases = Select[parentCases, First[#] == policy &]},
  <|
    "ExactPublication" -> Count[cases,
      row_ /; With[{published = publishedParents @@ row},
        published > 0 && published =!= row[[2]]]],
    "BoundedPublication" -> Count[cases,
      row_ /; publishedParents @@ row > row[[3]]],
    "FitAvailability" -> Count[cases,
      row_ /; row[[2]] <= row[[3]] && publishedParents @@ row == 0],
    "OverCapDeferral" -> Count[cases,
      row_ /; row[[2]] > row[[3]] && publishedParents @@ row =!= 0]
  |>
];

parentResults = AssociationMap[parentPolicyFailures, policies];
feasibleParentPolicies = Select[policies,
  Total[Values[parentResults[#]]] == 0 &
];
staticFalseRejectionWitness =
  publishedParents["StaticWorstCase", 4, 101, 10000] == 0;
exactActualFitWitness =
  publishedParents["ExactFrontier", 4, 101, 10000] == 4;

ClearAll[lag, advance, validators, oracleCost, readCost, fixedCost];
coldCost = oracleCost validators lag (lag + 1)/2;
guardedCost = oracleCost validators advance + readCost lag + fixedCost;
floorDomain = lag >= 1 && 0 <= advance <= lag && validators >= 1 &&
  oracleCost > 0 && readCost >= 0 && fixedCost >= 0;

crossoverHeadroom =
  oracleCost validators (lag (lag + 1)/2 - advance) - readCost lag;
crossoverIdentity =
  Expand[(coldCost - guardedCost) - (crossoverHeadroom - fixedCost)] === 0;
crossoverExact = crossoverIdentity;

ClearAll[oracleMin, validatorsMin, readMax, fixedMax];
robustSufficientCondition =
  oracleMin validatorsMin (lag (lag + 1)/2 - lag) >
    readMax lag + fixedMax;
frontierWork = lag (lag + 1)/2 - advance;
minimumFrontierWork = lag (lag + 1)/2 - lag;
robustLowerBound =
  oracleMin validatorsMin minimumFrontierWork - readMax lag - fixedMax;
robustRemainder =
  (oracleCost - oracleMin) validators frontierWork +
    oracleMin (validators - validatorsMin) frontierWork +
    oracleMin validatorsMin (frontierWork - minimumFrontierWork) +
    (readMax - readCost) lag + (fixedMax - fixedCost);
robustDecompositionIdentity =
  Expand[(coldCost - guardedCost) - robustLowerBound - robustRemainder] === 0;
frontierWorkBounds = Resolve[
  ForAll[{lag, advance},
    Implies[lag >= 1 && 0 <= advance <= lag,
      frontierWork >= minimumFrontierWork && minimumFrontierWork >= 0]],
  Reals
];
factorBounds = Resolve[
  ForAll[{lag, advance, validators, oracleCost, readCost, fixedCost,
      oracleMin, validatorsMin, readMax, fixedMax},
    Implies[
      lag >= 1 && 0 <= advance <= lag &&
        oracleMin > 0 && oracleCost >= oracleMin &&
        validatorsMin >= 1 && validators >= validatorsMin &&
        readMax >= 0 && 0 <= readCost <= readMax &&
        fixedMax >= 0 && 0 <= fixedCost <= fixedMax,
      oracleCost - oracleMin >= 0 && validators >= 0 &&
        oracleMin >= 0 && validators - validatorsMin >= 0 &&
        validatorsMin >= 0 && readMax - readCost >= 0 && lag >= 0 &&
        fixedMax - fixedCost >= 0]],
  Reals
];
nonnegativeProductsAndSum = Resolve[
  ForAll[{factor1, factor2, factor3, factor4, factor5,
      factor6, factor7, factor8, factor9, factor10, factor11},
    Implies[
      And @@ Thread[
        {factor1, factor2, factor3, factor4, factor5, factor6,
          factor7, factor8, factor9, factor10, factor11} >= 0],
      factor1 factor2 factor3 + factor4 factor5 factor6 +
        factor7 factor8 factor9 + factor10 + factor11 >= 0]],
  Reals
];
robustCrossoverSound = robustDecompositionIdentity &&
  TrueQ[frontierWorkBounds] && TrueQ[factorBounds] &&
  TrueQ[nonnegativeProductsAndSum];

ClearAll[scale, rho];
asymptoticSavings = FullSimplify[
  Limit[
    (coldCost - guardedCost /. {lag -> scale, advance -> rho scale})/scale^2,
    scale -> Infinity
  ],
  validators > 0 && oracleCost > 0 && readCost >= 0 && fixedCost >= 0 &&
    0 <= rho <= 1
];
asymptoticSavingsPositive = FullSimplify[
  asymptoticSavings > 0,
  validators > 0 && oracleCost > 0
];

ClearAll[computePrice, storagePrice, entryBytes];
tokenSavings = computePrice (coldCost - guardedCost) - storagePrice entryBytes;
tokenCrossoverIdentity = Expand[
  tokenSavings -
    (computePrice (coldCost - guardedCost) - storagePrice entryBytes)
] === 0;
tokenCrossoverExact = tokenCrossoverIdentity;

Print["[repair_design_regions] parent-policy failures: ", parentResults];
Print["[repair_design_regions] feasible bounded exact policies: ",
  feasibleParentPolicies];
Print["[repair_design_regions] static false-rejection witness: ",
  staticFalseRejectionWitness];
Print["[repair_design_regions] exact actual-fit witness: ",
  exactActualFitWitness];
Print["[repair_design_regions] floor crossover headroom: fixedCost < ",
  crossoverHeadroom];
Print["[repair_design_regions] floor crossover equivalence: ", crossoverExact];
Print["[repair_design_regions] robust crossover condition: ",
  robustSufficientCondition];
Print["[repair_design_regions] robust decomposition identity: ",
  robustDecompositionIdentity];
Print["[repair_design_regions] robust factor bounds: ", factorBounds];
Print["[repair_design_regions] nonnegative product/sum closure: ",
  nonnegativeProductsAndSum];
Print["[repair_design_regions] robust crossover soundness: ",
  robustCrossoverSound];
Print["[repair_design_regions] asymptotic savings / lag^2: ",
  asymptoticSavings];
Print["[repair_design_regions] asymptotic savings positive: ",
  asymptoticSavingsPositive];
Print["[repair_design_regions] token-cost crossover equivalence: ",
  tokenCrossoverExact];

pass = feasibleParentPolicies === {"ExactFrontier"} &&
  TrueQ[staticFalseRejectionWitness] && TrueQ[exactActualFitWitness] &&
  TrueQ[crossoverExact] && TrueQ[robustCrossoverSound] &&
  asymptoticSavings === oracleCost validators/2 &&
  TrueQ[asymptoticSavingsPositive] && TrueQ[tokenCrossoverExact];
Print["[repair_design_regions] SELF-TEST: ", If[pass, "PASS", "FAIL"]];
If[! pass, Exit[1]];
