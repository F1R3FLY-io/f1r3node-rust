(* ::Package:: *)

(* delta_ratchet.wl - Finality-lag ratchet model for the finalized-floor multi-parent merge.

   Models: casper/src/rust/finality/floor.rs (parent_frontier, uncached per-merge floor walk)
           casper/src/rust/util/rholang/interpreter_util.rs:746-748,1114-1179
             (MAX_FLOOR_DISTANCE_BLOCKS=256, MAX_PARENT_MERGE_SCOPE_BLOCKS=512, and the
              silent single-parent fallback that drops sibling-parent writes once the lag
              crosses the cap).

   Claim: the O(Delta^2) uncached floor walk makes the finalization-lag dynamics
   STRUCTURALLY UNSTABLE (every equilibrium is a repeller), so under sustained load the
   lag Delta = num(tip) - num(floor) inevitably runs away past the 256 cliff, which fires
   the write-dropping fallback (the "~400 blocks" symptom). Making the floor walk O(1)
   (cached / incremental) removes the destabilizing feedback and the system is stable.

   This is a SUPPORTING witness (not proof authority - see README). The exact,
   parameter-free safety violation is in formal/tlaplus/finalized_floor and the Rocq
   theories. Run:  wolfram -script formal/wolfram/finalized_floor/delta_ratchet.wl
                   (or: math -script ...); pass the flag  --self-test  is implicit (always
   prints PASS/FAIL and exits nonzero on FAIL). *)

(* ---- Model -------------------------------------------------------------------------- *)
(* Per round: tip advances by 1. The finalizer has per-round work budget B. The propose
   re-derives the floor at cost overhead(Delta); finality receives (B - overhead) work and
   advances the floor by advance(Delta) = (B - overhead(Delta))/w blocks (w = per-block
   certification cost). Delta_{n+1} = max(0, Delta_n + 1 - advance(Delta_n)). *)

ClearAll[aBuggy, aFixed, stepBuggy, stepFixed, fBuggy];
aBuggy[d_, B_, w_, k_] := Max[0, (B - k*d^2)/w];   (* uncached: overhead = k Delta^2 *)
aFixed[d_, B_, w_, c_] := Max[0, (B - c)/w];        (* cached O(1): overhead = const c  *)
stepBuggy[d_, B_, w_, k_] := Max[0, d + 1 - aBuggy[d, B, w, k]];
stepFixed[d_, B_, w_, c_] := Max[0, d + 1 - aFixed[d, B, w, c]];

(* ---- Parameter-INDEPENDENT structural instability (decided over the reals) ---------- *)
ClearAll[Ba, wa, ka, ca, da];
advanceBuggySym[da_] := (Ba - ka*da^2)/wa;          (* smooth advance, before the Max clamp *)
advanceFixedSym[da_] := (Ba - ca)/wa;
fBuggySym[da_] := da + 1 - advanceBuggySym[da];      (* the return map, smooth branch *)

buggyPositiveFeedback =
  Resolve[ForAll[{Ba, wa, ka, da},
    Implies[wa > 0 && ka > 0 && da > 0, D[advanceBuggySym[da], da] < 0]], Reals];
buggyEveryFixedPointUnstable =
  Resolve[ForAll[{Ba, wa, ka, da},
    Implies[wa > 0 && ka > 0 && da > 0, D[fBuggySym[da], da] > 1]], Reals];
fixedZeroFeedback =
  Resolve[ForAll[{Ba, wa, ca, da}, D[advanceFixedSym[da], da] == 0], Reals];

(* ---- Numeric demonstration (illustrative parameters; the dichotomy above is general) - *)
B0 = 5000.; w0 = 10.; k0 = 0.1; c0 = 10.; cliff = 256;
dStar = da /. Solve[(B0 - k0*da^2)/w0 == 1 && da > 0, da][[1]];   (* unstable tipping pt *)
transient = Ceiling[dStar] + 5;                                    (* a load spike above it *)
buggyTraj = NestList[stepBuggy[#, B0, w0, k0] &, N[transient], 600];
fixedTraj = NestList[stepFixed[#, B0, w0, c0] &, N[transient], 600];
roundsToBreach = First[FirstPosition[buggyTraj, x_ /; x > cliff]] - 1;
buggyDeltaAt400 = buggyTraj[[400]];
fixedMaxDelta = Max[fixedTraj];
fixedFinalDelta = Round[Last[fixedTraj], 0.01];

(* ---- Report + self-test ------------------------------------------------------------- *)
Print["[delta_ratchet] structural results (parameter-independent, Resolve over Reals):"];
Print["  buggy advance strictly decreasing in Delta (positive feedback): ", buggyPositiveFeedback];
Print["  buggy every equilibrium unstable (return-map slope > 1):        ", buggyEveryFixedPointUnstable];
Print["  fixed (O(1) floor) zero Delta-feedback (stable):                ", fixedZeroFeedback];
Print["[delta_ratchet] numeric demonstration (B=", B0, ", w=", w0, ", k=", k0, "):"];
Print["  unstable tipping point Delta* = ", dStar];
Print["  BUGGY: from transient ", transient, " -> breaches 256 in ", roundsToBreach,
      " rounds; Delta at round 400 = ", buggyDeltaAt400];
Print["  FIXED: from same transient -> max Delta = ", fixedMaxDelta,
      ", converges to ", fixedFinalDelta];

pass =
  TrueQ[buggyPositiveFeedback] && TrueQ[buggyEveryFixedPointUnstable] &&
  TrueQ[fixedZeroFeedback] && (buggyDeltaAt400 > cliff) && (fixedFinalDelta == 0.);
Print["[delta_ratchet] SELF-TEST: ", If[pass, "PASS", "FAIL"]];
If[! pass, Exit[1]];
