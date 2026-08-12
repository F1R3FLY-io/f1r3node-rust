(* depth-ceilings-dumbbell.wl — Figure S2 of the stack-safety technical report.
   Dumbbell plot (log-10 position axis) of the maximum survivable nesting depth
   before (slate) and after (green) conversion. Censored endpoints — depths at
   which the instrument stopped probing without finding a failure — are drawn
   with an amber arrowhead and a ">=" label; the env_get_deploy row is the
   experiment's CONTROL and moves essentially nowhere, exactly as predicted.

   Data source  : docs/technical-reports/stack-safety/data/depth-ceilings.tsv
                  (curated from the ledger; provenance machine-checked by
                  tools/mk_tables.py provenance)
   Regenerate   : wolframscript -file depth-ceilings-dumbbell.wl   (writes depth-ceilings-dumbbell.svg)
   Fallback     : any renderer may substitute matplotlib; keep filenames identical.
   Palette      : ledger Appendix E.1 — slate #455A64 = before, green #2E7D32 =
                  after, amber #EF6C00 = censoring marks. Censoring is encoded by
                  SHAPE (arrowhead) and TEXT (">="), never colour-alone; the
                  slate/green pair passes the CVD check (deutan dE 12.2).
*)

scriptDir = DirectoryName[$InputFileName];
repoRoot  = FileNameDrop[scriptDir, -4];
dataFile  = FileNameJoin[{repoRoot, "docs", "technical-reports", "stack-safety",
  "data", "depth-ceilings.tsv"}];

slate = RGBColor["#455A64"]; green = RGBColor["#2E7D32"]; amber = RGBColor["#EF6C00"];
inkPrimary = GrayLevel[0.15]; inkSecondary = GrayLevel[0.38];
gridCol = GrayLevel[0.93]; connCol = GrayLevel[0.62];

raw  = Import[dataFile, "TSV"];
rows = Select[raw, Length[#] >= 7 && StringQ[#[[1]]] && ! StringStartsQ[#[[1]], "#"] &&
    ! StringStartsQ[#[[1]], "label"] &];

fmtN[x_] := ToString[NumberForm[Round[x], DigitBlock -> 3, NumberSeparator -> ","]];

(* control_depth may be numeric, a numeric string (the TSV importer types the
   whole column as strings because it also holds a range and an empty cell),
   a "31\[Dash]33"-style range, or empty/Missing *)
controlPos[c_] := Which[
   NumberQ[c], {N@Log10[c], fmtN[c]},
   StringQ[c] && StringMatchQ[c, NumberString], controlPos[ToExpression[c]],
   StringQ[c] && StringMatchQ[c, NumberString ~~ ("\[Dash]" | "-" | "\[LongDash]" | "\:2013") ~~ NumberString],
   Module[{parts = StringSplit[c, "\[Dash]" | "-" | "\[LongDash]" | "\:2013"]},
    {N@Log10[Mean[ToExpression /@ parts]], c}],
   True, {None, ""}];

(* row order: most dramatic lift first (top) *)
order = {"inj_attempt metering handshake", "gRPC ingress teardown",
   "node JSON RhoExpr boundary", "generated Par decode (prost reader)",
   "plain_deploy end-to-end (2 MiB worker)", "env_get_deploy end-to-end"};
sorted = SortBy[rows, Position[order, #[[1]]] &];
n = Length[sorted];
yOf[i_] := n - i + 1;

elems = Table[Module[{r = sorted[[i]], y, cpos, clab, t, tx, censored, crowded},
    y = yOf[i]; t = r[[3]]; tx = N@Log10[t];
    {cpos, clab} = controlPos[r[[2]]];
    censored = r[[4]] === "yes";
    crowded = cpos =!= None && Abs[cpos - tx] < 0.18;
    {If[cpos =!= None,
      {{connCol, AbsoluteThickness[1.4], Line[{{cpos, y}, {tx, y}}]},
       {slate, PointSize[0.011], Point[{cpos, y}]},
       If[crowded,
        Text[Style[clab, 10.5, slate, Bold], {cpos - 0.07, y}, {1, 0}],
        Text[Style[clab, 10.5, slate, Bold], {cpos, y + 0.34}, {0, 0}]]}, {}],
     {green, PointSize[0.011], Point[{tx, y}]},
     Which[
      censored,
      {{amber, AbsoluteThickness[1.8], Arrowheads[0.020], Arrow[{{tx + 0.03, y}, {tx + 0.22, y}}]},
       Text[Style["\[GreaterEqual] " <> fmtN[t], 10.5, green, Bold], {tx, y + 0.34}, {0, 0}]},
      crowded,
      Text[Style[fmtN[t], 10.5, green, Bold], {tx + 0.07, y}, {-1, 0}],
      True,
      Text[Style[fmtN[t], 10.5, green, Bold], {tx, y + 0.34}, {0, 0}]]}],
   {i, n}];

leftTicks = Table[{yOf[i], Style[sorted[[i, 1]], 10.5, inkSecondary]}, {i, n}];
tickPow[lo_, hi_] := Table[{k, Superscript[10, k]}, {k, lo, hi}];

fig = Graphics[{
    {gridCol, Thin, Table[Line[{{1.35, y}, {6.35, y}}], {y, n}]},
    elems,
    Text[Style["slate = before, green = after; amber arrow + \[GreaterEqual] = censored\n(the instrument stopped probing without finding a failure)",
      Italic, 11, inkSecondary], {2.35, 4.62}, {0, 0}],
    Text[Style["the control moved \[TildeTilde]1\[Times], as predicted", Italic, 11, inkSecondary],
     {3.62, yOf[6]}, {0, 0}]
  },
  PlotRange -> {{1.35, 6.5}, {0.3, n + 1.15}},
  Frame -> {{True, False}, {True, False}},
  FrameStyle -> Directive[GrayLevel[0.55], AbsoluteThickness[0.8]],
  FrameTicks -> {{leftTicks, None}, {tickPow[2, 6], None}},
  FrameLabel -> {{None, None},
    {Style["maximum survivable nesting depth (log scale)", 12.5, inkPrimary], None}},
  Background -> White,
  BaseStyle -> {FontFamily -> "DejaVu Sans", FontColor -> inkPrimary},
  ImageSize -> {880, 430}, AspectRatio -> Full,
  ImagePadding -> {{235, 18}, {44, 10}}];

svgOut = Export[FileNameJoin[{scriptDir, "depth-ceilings-dumbbell.svg"}], fig];
Print["written: ", svgOut, " (", FileByteCount[svgOut], " bytes); rows ", n];
