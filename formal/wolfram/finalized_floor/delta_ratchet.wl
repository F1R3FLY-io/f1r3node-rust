(* ::Package:: *)

(* delta_ratchet.wl - Finality-lag service-rate exploration.

   The model separates arrival rate r from finalization service rate a. The
   historical lag-dependent floor walk reduces service as lag grows and creates
   positive feedback on its smooth branch. Constant-overhead floor discovery
   removes that lag-dependent feedback, but it does not by itself guarantee
   liveness: lag drains only when service exceeds arrivals, remains constant at
   equality, and grows when service is lower.

   This is a supporting exploratory witness. Rocq and TLA+/Apalache remain the
   correctness authorities. Run only when the licensed tier is selected:
     RUN_WOLFRAM=1 scripts/check-finalized-floor-ALL.sh
*)

ClearAll["Global`*"];

aBuggy[d_, budget_, work_, coefficient_] :=
  Max[0, (budget - coefficient d^2)/work];
aFixed[budget_, work_, constant_] := Max[0, (budget - constant)/work];
stepBuggy[d_, arrivals_, budget_, work_, coefficient_] :=
  Max[0, d + arrivals - aBuggy[d, budget, work, coefficient]];
stepFixed[d_, arrivals_, budget_, work_, constant_] :=
  Max[0, d + arrivals - aFixed[budget, work, constant]];

ClearAll[budget, work, coefficient, constant, lag, arrivals, service];
advanceBuggySmooth[lag_] := (budget - coefficient lag^2)/work;
returnBuggySmooth[lag_] := lag + arrivals - advanceBuggySmooth[lag];

buggyPositiveFeedback = Resolve[
  ForAll[{budget, work, coefficient, lag},
    Implies[work > 0 && coefficient > 0 && lag > 0,
      D[advanceBuggySmooth[lag], lag] < 0]],
  Reals
];
buggySmoothSlopeGreaterThanOne = Resolve[
  ForAll[{budget, work, coefficient, lag, arrivals},
    Implies[work > 0 && coefficient > 0 && lag > 0,
      D[returnBuggySmooth[lag], lag] > 1]],
  Reals
];
fixedZeroLagFeedback = Resolve[
  ForAll[{service, lag}, D[service, lag] == 0],
  Reals
];

fixedOverprovisionedDrains = Resolve[
  ForAll[{service, lag, arrivals},
    Implies[
      lag > 0 && arrivals >= 0 && service >= 0 && service > arrivals,
      Max[0, lag + arrivals - service] < lag]],
  Reals
];
fixedBalancedHolds = Resolve[
  ForAll[{service, lag, arrivals},
    Implies[
      lag >= 0 && arrivals >= 0 && service == arrivals,
      Max[0, lag + arrivals - service] == lag]],
  Reals
];
fixedUnderprovisionedGrows = Resolve[
  ForAll[{service, lag, arrivals},
    Implies[
      lag >= 0 && arrivals >= 0 && service >= 0 && service < arrivals,
      Max[0, lag + arrivals - service] > lag]],
  Reals
];

healthyBudget = 5000;
healthyWork = 10;
quadraticCoefficient = 1/10;
constantOverhead = 10;
arrivalRate = 1;
cliff = 256;
tippingPoint = lag /. First[Solve[
  (healthyBudget - quadraticCoefficient lag^2)/healthyWork == arrivalRate &&
    lag > 0,
  lag,
  Reals
]];
transient = Ceiling[tippingPoint] + 5;
buggyTrajectory = NestList[
  stepBuggy[#, arrivalRate, healthyBudget, healthyWork,
    quadraticCoefficient] &,
  transient,
  600
];
healthyFixedTrajectory = NestList[
  stepFixed[#, arrivalRate, healthyBudget, healthyWork, constantOverhead] &,
  transient,
  600
];
balancedConstant = healthyBudget - healthyWork arrivalRate;
balancedFixedTrajectory = NestList[
  stepFixed[#, arrivalRate, healthyBudget, healthyWork, balancedConstant] &,
  transient,
  40
];
overloadedBudget = 20;
overloadedWork = 20;
overloadedConstant = 10;
overloadedFixedTrajectory = NestList[
  stepFixed[#, arrivalRate, overloadedBudget, overloadedWork,
    overloadedConstant] &,
  0,
  40
];

roundsToBreach = First[FirstPosition[buggyTrajectory, value_ /; value > cliff]] - 1;
buggyLagAt400 = buggyTrajectory[[401]];
healthyFixedFinalLag = Last[healthyFixedTrajectory];
balancedFixedFinalLag = Last[balancedFixedTrajectory];
overloadedFixedFinalLag = Last[overloadedFixedTrajectory];

Print["[delta_ratchet] buggy service decreases with lag: ",
  buggyPositiveFeedback];
Print["[delta_ratchet] buggy smooth return slope exceeds one: ",
  buggySmoothSlopeGreaterThanOne];
Print["[delta_ratchet] constant overhead removes lag feedback: ",
  fixedZeroLagFeedback];
Print["[delta_ratchet] service > arrivals drains positive lag: ",
  fixedOverprovisionedDrains];
Print["[delta_ratchet] service = arrivals preserves lag: ",
  fixedBalancedHolds];
Print["[delta_ratchet] service < arrivals grows lag: ",
  fixedUnderprovisionedGrows];
Print["[delta_ratchet] tipping point: ", tippingPoint];
Print["[delta_ratchet] buggy transient ", transient,
  " breaches ", cliff, " after ", roundsToBreach,
  " rounds; lag at round 400 = ", buggyLagAt400];
Print["[delta_ratchet] overprovisioned constant-overhead final lag: ",
  healthyFixedFinalLag];
Print["[delta_ratchet] balanced constant-overhead final lag: ",
  balancedFixedFinalLag];
Print["[delta_ratchet] underprovisioned constant-overhead final lag: ",
  overloadedFixedFinalLag];

pass = TrueQ[buggyPositiveFeedback] &&
  TrueQ[buggySmoothSlopeGreaterThanOne] &&
  TrueQ[fixedZeroLagFeedback] &&
  TrueQ[fixedOverprovisionedDrains] && TrueQ[fixedBalancedHolds] &&
  TrueQ[fixedUnderprovisionedGrows] && roundsToBreach > 0 &&
  buggyLagAt400 > cliff && healthyFixedFinalLag == 0 &&
  balancedFixedFinalLag == transient && overloadedFixedFinalLag > 0;
Print["[delta_ratchet] SELF-TEST: ", If[pass, "PASS", "FAIL"]];
If[! pass, Exit[1]];
