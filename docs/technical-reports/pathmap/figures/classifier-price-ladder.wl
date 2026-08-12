(* classifier-price-ladder.wl — Figure P3 of the PathMap technical report.
   Log-log line of the stack-safe canonical-key classifier's speedup ratio
   (converted / recursive) against nesting depth: 0.264x at depth 1 (a real
   3.79x slowdown on the production-common shallow case), crossover before
   depth 8, and 233.874x at depth 1,024. The sub-1x region is shaded amber
   and labelled — the report publishes the cost beside the win.

   Data source  : docs/technical-reports/pathmap/data/classifier-ladder.tsv
                  (curated from the ledger §5.5; provenance machine-checked by
                  tools/mk_tables.py provenance)
   Regenerate   : wolframscript -file classifier-price-ladder.wl   (writes classifier-price-ladder.svg)
   Fallback     : any renderer may substitute matplotlib; keep filenames identical.
   Palette      : ledger Appendix E.1 — green #2E7D32 = the converted classifier's
                  ratio, slate #455A64 = the parity reference (dashed, labelled),
                  amber #EF6C00 tint = the pay-here region (labelled). Nothing is
                  colour-alone: the region and both lines carry text.
*)

scriptDir = DirectoryName[$InputFileName];
repoRoot  = FileNameDrop[scriptDir, -4];
dataFile  = FileNameJoin[{repoRoot, "docs", "technical-reports", "pathmap",
  "data", "classifier-ladder.tsv"}];

slate = RGBColor["#455A64"]; green = RGBColor["#2E7D32"]; amber = RGBColor["#EF6C00"];
amberTint = RGBColor[1, 0.925, 0.85];
inkPrimary = GrayLevel[0.15]; inkSecondary = GrayLevel[0.38];
gridCol = GrayLevel[0.93];

raw  = Import[dataFile, "TSV"];
rows = Select[raw, Length[#] >= 5 && NumberQ[#[[1]]] && NumberQ[#[[2]]] &];
pts  = {Log10[N@#[[1]]], Log10[N@#[[2]]]} & /@ rows;

fmtD[x_] := ToString[NumberForm[x, {6, 3}]];

(* crossover of the piecewise-log-linear interpolation with ratio = 1 *)
seg = SelectFirst[Partition[pts, 2, 1], #[[1, 2]] < 0 <= #[[2, 2]] &];
xCross = seg[[1, 1]] + (0 - seg[[1, 2]]) (seg[[2, 1]] - seg[[1, 1]])/(seg[[2, 2]] - seg[[1, 2]]);

fig = Graphics[{
    (* the pay-here region: ratio < 1 *)
    {amberTint, Rectangle[{-0.12, -0.72}, {3.12, 0}]},
    Text[Style["converted classifier pays here (ratio < 1):\n3.79\[Times] slower at depth 1 \[LongDash] the production-common case",
      Italic, 11, amber], {2.12, -0.33}, {0, 0}],
    {gridCol, Thin, Table[Line[{{-0.12, k}, {3.12, k}}], {k, -0.5, 2.5, 0.5}]},
    (* parity reference *)
    {slate, Dashed, AbsoluteThickness[1.2], Line[{{-0.12, 0}, {3.12, 0}}]},
    Text[Style["parity: converted = recursive (1\[Times])", 10.5, slate, Bold], {2.42, 0.11}, {0, 0}],
    (* the ladder *)
    {green, AbsoluteThickness[2.0], Line[pts]},
    {green, PointSize[0.012], Point[pts]},
    Table[Text[Style[fmtD[rows[[i, 2]]] <> "\[Times]", 11.5, green, Bold],
      {pts[[i, 1]], pts[[i, 2]] + 0.17}, {0, 0}], {i, Length[pts]}],
    (* crossover annotation *)
    {inkSecondary, Dashed, AbsoluteThickness[0.9], Line[{{xCross, -0.72}, {xCross, 0}}]},
    Text[Style["crossover \[TildeTilde] depth " <>
       ToString[NumberForm[10^xCross, {3, 1}]] <> ", before depth 8", Italic, 10.5, inkSecondary],
     {xCross + 0.035, -0.64}, {-1, 0}]
  },
  PlotRange -> {{-0.12, 3.12}, {-0.72, 2.62}},
  Frame -> {{True, False}, {True, False}},
  FrameStyle -> Directive[GrayLevel[0.55], AbsoluteThickness[0.8]],
  FrameTicks -> {{Table[{k, Superscript[10, k]}, {k, 0, 2}], None},
    {Table[{Log10[d], ToString[d]}, {d, {1, 8, 64, 1024}}], None}},
  FrameLabel -> {{Style["speedup ratio, converted \[Divide] recursive (log scale)", 12.5, inkPrimary], None},
    {Style["nesting depth (log scale)", 12.5, inkPrimary], None}},
  Background -> White,
  BaseStyle -> {FontFamily -> "DejaVu Sans", FontColor -> inkPrimary},
  ImageSize -> 780, AspectRatio -> 0.62,
  ImagePadding -> {{64, 14}, {44, 10}}];

svgOut = Export[FileNameJoin[{scriptDir, "classifier-price-ladder.svg"}], fig];
Print["written: ", svgOut, " (", FileByteCount[svgOut], " bytes); crossover 10^",
  NumberForm[xCross, {4, 3}], " = depth ", NumberForm[10^xCross, {4, 2}]];
