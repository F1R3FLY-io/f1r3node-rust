(* time-before-after.wl — Figure S5 of the stack-safety technical report.
   Dumbbell plot (log-10 position axis, seconds) of the wall/CPU-time wins:
   the SS-G45 GSS boundary workload (22.94x), the SS-B2 tokio fire-and-forget
   driver (~3.2x less CPU for the same contract), and the SS-G1 chain_2000
   clone (39x). The companion work-counter fact — 336,387,001 GSS ancestry
   visits eliminated to 0 — is a table fact in the report body; a log axis
   cannot plot its zero.

   Data source  : docs/technical-reports/stack-safety/data/time-wins.tsv
                  (curated from the ledger; provenance machine-checked by
                  tools/mk_tables.py provenance)
   Regenerate   : wolframscript -file time-before-after.wl   (writes time-before-after.svg)
   Fallback     : any renderer may substitute matplotlib; keep filenames identical.
   Palette      : ledger Appendix E.1 — slate #455A64 = before, green #2E7D32 =
                  after (CVD-checked pair, deutan dE 12.2); every dot is labelled.
*)

scriptDir = DirectoryName[$InputFileName];
repoRoot  = FileNameDrop[scriptDir, -4];
dataFile  = FileNameJoin[{repoRoot, "docs", "technical-reports", "stack-safety",
  "data", "time-wins.tsv"}];

slate = RGBColor["#455A64"]; green = RGBColor["#2E7D32"];
inkPrimary = GrayLevel[0.15]; inkSecondary = GrayLevel[0.38];
gridCol = GrayLevel[0.93]; connCol = GrayLevel[0.62];

raw  = Import[dataFile, "TSV"];
rows = Select[raw, Length[#] >= 8 && StringQ[#[[1]]] && ! StringStartsQ[#[[1]], "#"] &&
    ! StringStartsQ[#[[1]], "label"] &&
    NumberQ[#[[2]]] && NumberQ[#[[3]]] && #[[2]] > 0 && #[[3]] > 0 &&
    MemberQ[{"s", "s CPU"}, ToString[#[[4]]]] &];  (* seconds rows only — the
      336,387,001 -> 0 ancestry-visit counter is a table fact; log has no zero *)

trimZeros[s_String] := StringReplace[s, RegularExpression["\\.?0+$"] -> ""];
fmtS[x_] := trimZeros[ToString[NumberForm[x, {6, 2}]]] <> " s";
fmtR[x_] := trimZeros[ToString[NumberForm[x, {6, 2}]]] <> "\[Times] faster";

n = Length[rows];
yOf[i_] := n - i + 1;

elems = Table[Module[{r = rows[[i]], y, cx, tx, tight},
    y = yOf[i];
    cx = Log10[N@r[[2]]]; tx = Log10[N@r[[3]]];
    tight = Abs[cx - tx] < 0.75;  (* close dots: control label goes beside its
      dot instead of above it, so the two labels cannot collide *)
    {{connCol, AbsoluteThickness[1.4], Line[{{tx, y}, {cx, y}}]},
     {slate, PointSize[0.011], Point[{cx, y}]},
     If[tight,
      Text[Style[fmtS[N@r[[2]]] <> If[r[[4]] === "s CPU", " CPU", ""], 10.5, slate, Bold],
       {cx + 0.06, y}, {-1, 0}],
      Text[Style[fmtS[N@r[[2]]] <> If[r[[4]] === "s CPU", " CPU", ""], 10.5, slate, Bold],
       {cx, y + 0.30}, {0, 0}]],
     {green, PointSize[0.011], Point[{tx, y}]},
     Text[Style[fmtS[N@r[[3]]] <> If[r[[4]] === "s CPU", " CPU", ""], 10.5, green, Bold],
      {tx, y + 0.30}, {0, 0}],
     If[NumberQ[r[[5]]],
      Text[Style[fmtR[N@r[[5]]], 11, inkPrimary, Bold], {(cx + tx)/2, y - 0.30}, {0, 0}],
      Nothing]}],
   {i, n}];

leftTicks = Table[{yOf[i], Style[rows[[i, 1]], 10.5, inkSecondary]}, {i, n}];

fig = Graphics[{
    {gridCol, Thin, Table[Line[{{-1.4, y}, {2.85, y}}], {y, n}]},
    elems
  },
  PlotRange -> {{-1.4, 2.95}, {0.3, n + 0.85}},
  Frame -> {{True, False}, {True, False}},
  FrameStyle -> Directive[GrayLevel[0.55], AbsoluteThickness[0.8]],
  FrameTicks -> {{leftTicks, None},
    {Table[{k, Superscript[10, k]}, {k, -1, 2}], None}},
  FrameLabel -> {{None, None}, {Style["elapsed time (s, log scale)", 12.5, inkPrimary], None}},
  Background -> White,
  BaseStyle -> {FontFamily -> "DejaVu Sans", FontColor -> inkPrimary},
  ImageSize -> {880, 300}, AspectRatio -> Full,
  ImagePadding -> {{235, 18}, {44, 10}}];

svgOut = Export[FileNameJoin[{scriptDir, "time-before-after.svg"}], fig];
Print["written: ", svgOut, " (", FileByteCount[svgOut], " bytes); rows ", n];
