(* ::Package:: *)

(* weighted_quorum_regions.wl - Exact weighted-finality parameter exploration.

   This supporting witness mirrors ft_decides_exact in
   casper/src/rust/safety/clique_oracle.rs. It uses exact arithmetic to explore
   the strict clique inequality, the independent hard agreeing-majority gate,
   and accountable overlap across the complete threshold domain
   -den <= num <= den.

   Rocq is the proof authority. This licensed Wolfram tier is intentionally
   exploratory and opt-in:
     RUN_WOLFRAM=1 scripts/check-finalized-floor-ALL.sh
*)

ClearAll["Global`*"];

strictClique[q_, s_, num_, den_] := 2 q den > s (den + num);
inclusiveClique[q_, s_, num_, den_] := 2 q den >= s (den + num);
hardMajority[agreeing_, s_] := 2 agreeing > s;
decision[agreeing_, q_, s_, num_, den_] :=
  hardMajority[agreeing, s] && strictClique[q, s, num, den];
qMinimum[s_, num_, den_] := Floor[s (den + num)/(2 den)] + 1;

ClearAll[q, q1, q2, s, num, den, agreeing, faulty];
domain = s > 0 && den > 0 && -den <= num <= den && 0 <= q <= s;

strictRegion = Reduce[domain && strictClique[q, s, num, den], q, Reals];
strictRegionExact = Resolve[
  ForAll[{q, s, num, den},
    Implies[domain,
      Equivalent[strictClique[q, s, num, den],
        q > s (den + num)/(2 den)]]],
  Reals
];

nonnegativeThresholdImpliesCliqueMajority = Resolve[
  ForAll[{q, s, num, den},
    Implies[domain && num >= 0 && strictClique[q, s, num, den],
      2 q > s]],
  Reals
];

strictCertificatesForceAccountableOverlap = Resolve[
  ForAll[{q1, q2, s, num, den},
    Implies[
      s > 0 && den > 0 && -den <= num <= den &&
        0 <= q1 <= s && 0 <= q2 <= s &&
        strictClique[q1, s, num, den] &&
        strictClique[q2, s, num, den],
      Max[0, q1 + q2 - s] den > s num]],
  Reals
];

faultBudgetContradictsTwoIncompatibleCertificates = Resolve[
  ForAll[{q1, q2, s, num, den, faulty},
    Implies[
      s > 0 && den > 0 && 0 <= num <= den &&
        0 <= q1 <= s && 0 <= q2 <= s &&
        0 <= faulty <= s &&
        strictClique[q1, s, num, den] &&
        strictClique[q2, s, num, den] &&
        faulty >= Max[0, q1 + q2 - s],
      faulty den > s num]],
  Reals
];

denominators = {1, 2, 3, 4, 5, 8, 10, 16};
integerCases = Flatten[
  Table[{stake, denominator, numerator, clique},
    {stake, 1, 64},
    {denominator, denominators},
    {numerator, -denominator, denominator},
    {clique, 0, stake}],
  3
];

integerThresholdFailures = Select[integerCases,
  Function[row,
    With[{stake = row[[1]], denominator = row[[2]],
        numerator = row[[3]], clique = row[[4]]},
      strictClique[clique, stake, numerator, denominator] =!=
        (clique >= qMinimum[stake, numerator, denominator])
    ]
  ]
];

strictTieCases = Select[integerCases,
  Function[row,
    With[{stake = row[[1]], denominator = row[[2]],
        numerator = row[[3]], clique = row[[4]]},
      2 clique denominator == stake (denominator + numerator)
    ]
  ]
];
strictTieFailures = Select[strictTieCases,
  Function[row,
    strictClique[row[[4]], row[[1]], row[[3]], row[[2]]] ||
      ! inclusiveClique[row[[4]], row[[1]], row[[3]], row[[2]]]
  ]
];

nonnegativeMajorityFailures = Select[integerCases,
  Function[row,
    With[{stake = row[[1]], denominator = row[[2]],
        numerator = row[[3]], clique = row[[4]]},
      numerator >= 0 &&
        strictClique[clique, stake, numerator, denominator] &&
        2 clique <= stake
    ]
  ]
];

ppmDen = 1000000;
ppmNumerators = {-1000000, -500000, 0, 100000, 330000, 500000, 1000000};
ppmCases = Flatten[
  Table[{stake, numerator, clique},
    {stake, 1, 256},
    {numerator, ppmNumerators},
    {clique, 0, stake}],
  2
];
ppmFailures = Select[ppmCases,
  Function[row,
    strictClique[row[[3]], row[[1]], row[[2]], ppmDen] =!=
      (row[[3]] >= qMinimum[row[[1]], row[[2]], ppmDen])
  ]
];

inclusiveBoundaryWitness =
  inclusiveClique[8, 16, 0, 1] && ! strictClique[8, 16, 0, 1];
hardGateWitness =
  strictClique[1, 10, -10, 10] &&
    ! hardMajority[5, 10] && ! decision[5, 1, 10, -10, 10];
accountabilityPremiseWitness =
  strictClique[2, 3, 0, 1] &&
    Max[0, 2 + 2 - 3] == 1 &&
    ! (0 >= Max[0, 2 + 2 - 3]);
asymmetricStakeWitness =
  Total[{3, 1, 1, 1}] == 6 &&
    qMinimum[6, 1, 3] == 5 &&
    ! decision[6, 4, 6, 1, 3] && decision[6, 5, 6, 1, 3];

Print["[weighted_quorum_regions] strict q-region: ", strictRegion];
Print["[weighted_quorum_regions] symbolic strict-region equivalence: ",
  strictRegionExact];
Print["[weighted_quorum_regions] nonnegative threshold implies clique majority: ",
  nonnegativeThresholdImpliesCliqueMajority];
Print["[weighted_quorum_regions] strict certificates force accountable overlap: ",
  strictCertificatesForceAccountableOverlap];
Print["[weighted_quorum_regions] bounded-fault premise excludes two certificates: ",
  faultBudgetContradictsTwoIncompatibleCertificates];
Print["[weighted_quorum_regions] exact integer cases: ", Length[integerCases]];
Print["[weighted_quorum_regions] exact threshold failures: ",
  Length[integerThresholdFailures]];
Print["[weighted_quorum_regions] strict boundary ties: ", Length[strictTieCases]];
Print["[weighted_quorum_regions] strict boundary failures: ",
  Length[strictTieFailures]];
Print["[weighted_quorum_regions] nonnegative majority failures: ",
  Length[nonnegativeMajorityFailures]];
Print["[weighted_quorum_regions] ppm cases: ", Length[ppmCases]];
Print["[weighted_quorum_regions] ppm failures: ", Length[ppmFailures]];
Print["[weighted_quorum_regions] inclusive-boundary control: ",
  inclusiveBoundaryWitness];
Print["[weighted_quorum_regions] independent hard-gate control: ",
  hardGateWitness];
Print["[weighted_quorum_regions] accountability-premise control: ",
  accountabilityPremiseWitness];
Print["[weighted_quorum_regions] asymmetric-stake boundary: ",
  asymmetricStakeWitness];

pass = TrueQ[strictRegionExact] &&
  TrueQ[nonnegativeThresholdImpliesCliqueMajority] &&
  TrueQ[strictCertificatesForceAccountableOverlap] &&
  TrueQ[faultBudgetContradictsTwoIncompatibleCertificates] &&
  integerThresholdFailures === {} && strictTieCases =!= {} &&
  strictTieFailures === {} && nonnegativeMajorityFailures === {} &&
  ppmFailures === {} && TrueQ[inclusiveBoundaryWitness] &&
  TrueQ[hardGateWitness] && TrueQ[accountabilityPremiseWitness] &&
  TrueQ[asymmetricStakeWitness];
Print["[weighted_quorum_regions] SELF-TEST: ", If[pass, "PASS", "FAIL"]];
If[! pass, Exit[1]];
