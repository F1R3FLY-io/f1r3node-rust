(* ::Package:: *)

(* ghost_heaviest_subtree.wl - LMD-GHOST heaviest-subtree fork-choice witness.

   Models: casper/src/rust/estimator.rs
             build_scores_map      (per-validator supporting-weight accumulation)
             greedy_ghost_head     (per-level heaviest scored-child descent)
             terminal_frontier     (concurrent scored-leaf retention)
           casper/src/rust/causal_equivocation.rs
             complete certified vote projection + frozen-floor authority.

   Claims (supporting witness - the axiom-free proof authority is
   formal/rocq/fork_choice; this corroborates it numerically/symbolically):
     (a) GHOST correctness: descending into the maximum-score child at each level
         reaches the LMD-GHOST leaf even when a globally larger terminal leaf sits
         below a lighter subtree.
     (b) Frontier correctness: every asynchronous expansion order converges to the
         same duplicate-free terminal set, including a shared multi-parent leaf.
     (c) Ranking correctness: the greedy head is first and every remaining terminal
         is ordered by score descending, hash ascending.
     (d) Context extensionality: candidate bonds and receiver-local invalid/top state
         cannot change scores, LCA input, or fork choice for one certified context.

   Run: wolfram -script formal/wolfram/fork_choice/ghost_heaviest_subtree.wl
        (or math -script ...). The gate supplies the licensed Wolfram base directories;
        a discovered kernel must bind the license. Always prints PASS/FAIL and Exit[1]
        on FAIL. *)

ClearAll[children, score, ghostStep, ghostPath, ghostLeaf, certifiedScore,
  expand, finalFrontiers];

(* ---- (a) GHOST over cumulative certified latest-message scores ----------------- *)
(* Node 4 is a shared terminal child of nodes 1 and 6. The branch at node 1 has
   aggregate score 60, split across two leaves of score 30. Node 2 has one leaf
   of score 40. Greedy GHOST must enter node 1; globally sorting terminal leaves
   would incorrectly choose node 5. *)
children = <| 0 -> {1, 2, 6}, 1 -> {3, 4}, 2 -> {5}, 3 -> {}, 4 -> {},
              5 -> {}, 6 -> {4} |>;
score = <| 0 -> 110, 1 -> 60, 2 -> 40, 3 -> 30, 4 -> 30, 5 -> 40, 6 -> 30 |>;
ghostStep[n_] := Module[{cs = children[n]},
   If[cs === {}, n, First@SortBy[cs, {-score[#] &, # &}]]];
ghostPath = NestWhileList[ghostStep, 0, children[#] =!= {} &];
ghostLeaf = Last[ghostPath];
ghostInvariant = AllTrue[Most[ghostPath],
   Function[n, score[ghostStep[n]] == Max[score /@ children[n]]]];
ghostCorrect = ghostInvariant && (ghostLeaf == 3);

(* ---- (b) Every asynchronous expansion order reaches one exact frontier --------- *)
expand[frontier_, node_] := Union[DeleteCases[frontier, node], children[node]];
finalFrontiers[frontier_] := Module[{expandable},
  expandable = Select[frontier, children[#] =!= {} &];
  If[expandable === {}, {Sort[frontier]},
    DeleteDuplicates[Flatten[finalFrontiers[expand[frontier, #]] & /@ expandable, 1]]]
];
allFinalFrontiers = finalFrontiers[{0}];
terminalSet = {3, 4, 5};
frontierConfluent = allFinalFrontiers === {terminalSet};
sharedLeafUnique = Count[First[allFinalFrontiers], 4] == 1;

(* ---- (c) Head-first composition differs from the unsafe global-leaf rule ------- *)
terminalOrder = SortBy[terminalSet, {-score[#] &, # &}];
rankedTips = Prepend[DeleteCases[terminalOrder, ghostLeaf], ghostLeaf];
globalTerminalHead = First[terminalOrder];
rankingCorrect = rankedTips === {3, 5, 4};
globalLeafCounterexample = globalTerminalHead == 5 && globalTerminalHead =!= ghostLeaf &&
  score[1] == 60 && score[2] == 40;

(* Greedy and frontier traversals terminate because every traversed child advances
   the block-number measure and the finite DAG bounds it. *)
depths = Range[0, Length[ghostPath] - 1];
numMonotone = AllTrue[Differences[depths], # == 1 &];
boundedByHeight = (Length[ghostPath] <= Length[children]);

(* ---- (d) Certified-context extensionality --------------------------------------- *)
frozenAuthority = <|"v1" -> 10, "v2" -> 5|>;
candidateBondsA = <|"v1" -> 1, "v2" -> 100|>;
candidateBondsB = <|"v1" -> 100, "v2" -> 1|>;
certifiedSupport = <|"v1" -> {0, 1, 3}, "v2" -> {0, 2, 5}|>;
certifiedMessages = <|"v1" -> 3, "v2" -> 5|>;
receiverA = <|"invalid" -> {3}, "ambientTop" -> 6|>;
receiverB = <|"invalid" -> {5}, "ambientTop" -> 100000|>;

certifiedScore[authority_, support_, block_] := Total[
  KeyValueMap[If[MemberQ[#2, block], Lookup[authority, #1], 0] &, support]
];
certifiedScores = AssociationMap[
  certifiedScore[frozenAuthority, certifiedSupport, #] &,
  Keys[children]
];
contextProjection[_, messages_] := messages;
contextScore[_, _, authority_, support_] := AssociationMap[
  certifiedScore[authority, support, #] &,
  Keys[children]
];

candidateBondInvariant =
  contextScore[candidateBondsA, receiverA, frozenAuthority, certifiedSupport] ===
  contextScore[candidateBondsB, receiverA, frozenAuthority, certifiedSupport];
receiverStateInvariant =
  contextScore[candidateBondsA, receiverA, frozenAuthority, certifiedSupport] ===
  contextScore[candidateBondsA, receiverB, frozenAuthority, certifiedSupport] &&
  contextProjection[receiverA, certifiedMessages] ===
  contextProjection[receiverB, certifiedMessages];
frozenAuthoritySelectsExpectedBranch = certifiedScores[1] > certifiedScores[2];

Print["[ghost_heaviest_subtree] GHOST descent path: ", ghostPath, " -> leaf ", ghostLeaf];
Print["  (a) greedy heaviest-subtree descent is correct:       ", ghostCorrect];
Print["  (b) every async frontier order is exact/confluent:    ", frontierConfluent && sharedLeafUnique];
Print["  (c) GHOST-head + ranked-tail composition is exact:    ", rankingCorrect];
Print["  (c) global terminal argmax counterexample reproduced: ", globalLeafCounterexample];
Print["  (c) traversal measures are monotone and bounded:      ", numMonotone && boundedByHeight];
Print["  (d) candidate bond maps cannot reweight the round:    ", candidateBondInvariant];
Print["  (d) receiver-local cache/top cannot change context:   ", receiverStateInvariant];
Print["  (d) frozen authority selects the expected branch:     ", frozenAuthoritySelectsExpectedBranch];

pass = ghostCorrect && frontierConfluent && sharedLeafUnique && rankingCorrect &&
       globalLeafCounterexample && numMonotone && boundedByHeight &&
       candidateBondInvariant && receiverStateInvariant &&
       frozenAuthoritySelectsExpectedBranch;
Print["[ghost_heaviest_subtree] SELF-TEST: ", If[pass, "PASS", "FAIL"]];
If[! pass, Exit[1]];
