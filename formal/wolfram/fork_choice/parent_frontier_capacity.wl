(* ::Package:: *)

(* parent_frontier_capacity.wl - Exact frozen-parent-frontier witness.

   This supporting oracle enumerates every rooted five-block DAG, every
   candidate finalized floor, and every unique causal-latest-message subset
   produced by up to three validators. It mirrors the exact construction seam
   in casper/src/rust/engine/multi_parent_casper/snapshot.rs without replacing
   the authoritative Rocq proof or the concurrent TLA+/Apalache models.

   For C = unique causal tips and finalized floor F, the finality-vote tips V
   are exactly the members of C that equal or descend from F. The candidate set
   is C union {F} exactly when V is empty. Reachability compaction retains
   exactly the maximal elements. A valid designated GHOST head is a maximal
   member of V, or F when V is empty; it remains first and the remaining hashes
   are ascending. Capacity admits that entire ordered frontier iff it fits; no
   partial parent list is published on deferral.

   The script also requires witnesses for four unsafe alternatives: a static
   configured-validator maximum, omitted floor backstop, over-cap truncation,
   and skipped reachability compaction.

   Run only when a licensed kernel is intentionally selected:
     RUN_WOLFRAM=1 scripts/check-fork-choice-ALL.sh
*)

ClearAll["Global`*"];

blockCount = 5;
validatorCount = 3;
parentChoices = Table[Rest[Subsets[Range[child - 1]]],
  {child, 2, blockCount}];
dagParentSets = Tuples[parentChoices];
latestSubsets = Subsets[Range[blockCount], {0, validatorCount}];

ancestorMatrix[choices_] := Module[
  {ancestors = Table[ancestor == descendant,
    {ancestor, blockCount}, {descendant, blockCount}]},
  Do[
    Do[
      ancestors[[All, child]] =
        MapThread[Or, {ancestors[[All, child]], ancestors[[All, parent]]}],
      {parent, choices[[child - 1]]}
    ],
    {child, 2, blockCount}
  ];
  ancestors
];

frontierCandidates[ancestors_, causalTips_, floor_] := Module[
  {candidates = DeleteDuplicates[causalTips]},
  If[! AnyTrue[candidates, ancestors[[floor, #]] &],
    candidates = Append[candidates, floor]
  ];
  candidates
];

maximalElements[ancestors_, candidates_] := Select[candidates,
  Function[candidate,
    ! AnyTrue[DeleteCases[candidates, candidate],
      ancestors[[candidate, #]] &]
  ]
];

exactFrontier[ancestors_, causalTips_, floor_] :=
  maximalElements[ancestors,
    frontierCandidates[ancestors, causalTips, floor]];

pairwiseAntichainQ[ancestors_, frontier_] := AllTrue[
  Subsets[frontier, {2}],
  Function[pair,
    ! ancestors[[pair[[1]], pair[[2]]]] &&
      ! ancestors[[pair[[2]], pair[[1]]]]
  ]
];

coversQ[ancestors_, ancestorsToCover_, frontier_] := AllTrue[
  ancestorsToCover,
  Function[candidate,
    AnyTrue[frontier, ancestors[[candidate, #]] &]
  ]
];

canonicalParentOrder[frontier_, ghostHead_] :=
  Join[{ghostHead}, Sort[DeleteCases[frontier, ghostHead]]];

admitExactFrontier[orderedFrontier_, cap_] :=
  If[Length[orderedFrontier] <= cap,
    orderedFrontier,
    Missing["CapacityExceeded"]
  ];

headOrderCases = 0;
omittedFloorWitnesses = 0;
truncationWitnesses = 0;
uncompactedWitnesses = 0;

failures = Reap[
  Do[
    ancestors = ancestorMatrix[choices];
    Do[
      candidates = frontierCandidates[ancestors, causalTips, floor];
      expectedMaximal = maximalElements[ancestors, candidates];
      frontier = exactFrontier[ancestors, causalTips, floor];
      uniqueTips = DeleteDuplicates[causalTips];
      voteTips = Select[uniqueTips, ancestors[[floor, #]] &];

      If[frontier === {},
        Sow[{"empty-frontier", choices, floor, causalTips}]
      ];
      If[Sort[frontier] =!= Sort[expectedMaximal],
        Sow[{"not-exact-maximal-set", choices, floor, causalTips,
          expectedMaximal, frontier}]
      ];
      If[! pairwiseAntichainQ[ancestors, frontier],
        Sow[{"not-pairwise-antichain", choices, floor, causalTips,
          frontier}]
      ];
      If[! coversQ[ancestors, uniqueTips, frontier],
        Sow[{"causal-coverage", choices, floor, causalTips, frontier}]
      ];
      If[! coversQ[ancestors, {floor}, frontier],
        Sow[{"floor-coverage", choices, floor, causalTips, frontier}]
      ];
      If[Length[frontier] > Length[uniqueTips] + 1,
        Sow[{"cardinality-bound", choices, floor, causalTips, frontier}]
      ];

      permutedFrontiers = DeleteDuplicates[
        Sort /@ (exactFrontier[ancestors, #, floor] & /@
          Permutations[causalTips])
      ];
      If[Length[permutedFrontiers] =!= 1,
        Sow[{"frontier-permutation-invariance", choices, floor,
          causalTips, permutedFrontiers}]
      ];

      headCandidates = If[voteTips === {},
        {floor},
        maximalElements[ancestors, voteTips]
      ];
      If[! AllTrue[headCandidates, MemberQ[frontier, #] &],
        Sow[{"ghost-head-not-in-frontier", choices, floor, causalTips,
          voteTips, headCandidates, frontier}]
      ];

      Do[
        orderedFrontier = canonicalParentOrder[frontier, ghostHead];
        headOrderCases++;
        If[First[orderedFrontier] =!= ghostHead,
          Sow[{"ghost-head-not-first", choices, floor, causalTips,
            ghostHead, orderedFrontier}]
        ];
        If[Sort[orderedFrontier] =!= Sort[frontier],
          Sow[{"canonical-order-mutated-frontier", choices, floor,
            causalTips, ghostHead, orderedFrontier, frontier}]
        ];
        If[Rest[orderedFrontier] =!= Sort[Rest[orderedFrontier]],
          Sow[{"tail-not-hash-canonical", choices, floor, causalTips,
            ghostHead, orderedFrontier}]
        ];
        permutedOrders = DeleteDuplicates[
          canonicalParentOrder[
            exactFrontier[ancestors, #, floor], ghostHead] & /@
              Permutations[causalTips]
        ];
        If[Length[permutedOrders] =!= 1,
          Sow[{"ordered-permutation-invariance", choices, floor,
            causalTips, ghostHead, permutedOrders}]
        ],
        {ghostHead, headCandidates}
      ];

      designatedHead = First[headCandidates];
      orderedFrontier = canonicalParentOrder[frontier, designatedHead];
      Do[
        admission = admitExactFrontier[orderedFrontier, cap];
        If[(admission =!= Missing["CapacityExceeded"]) =!=
            (Length[orderedFrontier] <= cap),
          Sow[{"admission-equivalence", choices, floor, causalTips, cap,
            orderedFrontier, admission}]
        ];
        If[Length[orderedFrontier] <= cap &&
            admission =!= orderedFrontier,
          Sow[{"admission-mutated-frontier", choices, floor, causalTips,
            cap, orderedFrontier, admission}]
        ];
        If[Length[orderedFrontier] > cap &&
            admission =!= Missing["CapacityExceeded"],
          Sow[{"over-cap-published-parents", choices, floor, causalTips,
            cap, orderedFrontier, admission}]
        ],
        {cap, 1, blockCount + 1}
      ];

      If[! AnyTrue[uniqueTips, ancestors[[floor, #]] &],
        withoutFloor = maximalElements[ancestors, uniqueTips];
        If[! coversQ[ancestors, {floor}, withoutFloor],
          omittedFloorWitnesses++
        ]
      ];
      If[Length[orderedFrontier] > 1,
        Do[
          truncated = Take[orderedFrontier, cap];
          If[! coversQ[ancestors, uniqueTips, truncated] ||
              ! coversQ[ancestors, {floor}, truncated],
            truncationWitnesses++
          ],
          {cap, 1, Length[orderedFrontier] - 1}
        ]
      ];
      If[! pairwiseAntichainQ[ancestors, candidates],
        uncompactedWitnesses++
      ],
      {floor, Range[blockCount]},
      {causalTips, latestSubsets}
    ],
    {choices, dagParentSets}
  ]
][[2]];

failureList = If[failures === {}, {}, First[failures]];
frontierCases = Length[dagParentSets] blockCount Length[latestSubsets];
admissionCases = frontierCases (blockCount + 1);
staticMaximumCounterexample = 10000 + 1 > 101 && 4 <= 101;

Print["[parent_frontier_capacity] rooted DAGs:             ",
  Length[dagParentSets]];
Print["[parent_frontier_capacity] frontier cases:         ",
  frontierCases];
Print["[parent_frontier_capacity] head-order cases:       ",
  headOrderCases];
Print["[parent_frontier_capacity] cap admission cases:    ",
  admissionCases];
Print["[parent_frontier_capacity] invariant failures:     ",
  Length[failureList]];
Print["[parent_frontier_capacity] static-gate witness:     ",
  staticMaximumCounterexample];
Print["[parent_frontier_capacity] omitted-floor witnesses: ",
  omittedFloorWitnesses];
Print["[parent_frontier_capacity] truncation witnesses:    ",
  truncationWitnesses];
Print["[parent_frontier_capacity] uncompacted witnesses:   ",
  uncompactedWitnesses];
If[failureList =!= {},
  Print["[parent_frontier_capacity] first failure:          ",
    First[failureList]]
];

pass = failureList === {} && staticMaximumCounterexample &&
  omittedFloorWitnesses > 0 && truncationWitnesses > 0 &&
  uncompactedWitnesses > 0;
Print["[parent_frontier_capacity] SELF-TEST: ",
  If[pass, "PASS", "FAIL"]];
If[! pass, Exit[1]];
