(* subject-depth-histogram.wl — Figure S6 of the stack-safety technical report.
   Lollipop chart (log-10 count axis) of the nesting depths observed at the
   production cold-store encode boundary (the bincode_encoder subject of the
   Phase-7 depth census), aggregated over root kinds. Production terms are
   shallow — the eliminated depth slopes govern adversarial nesting, not the
   common case. Position (dots) carries the log-scale value, never bar length,
   and every dot is labelled with its exact count.

   Data source  : docs/design/stack-safety/measurements/phase7-depth-histograms-2026-08-03.tsv
                  (companion denominators: phase7-depth-corpora-2026-08-03.tsv)
   Regenerate   : wolframscript -file subject-depth-histogram.wl   (writes subject-depth-histogram.svg)
   Fallback     : any renderer may substitute matplotlib; keep filenames identical.
   Palette      : ledger Appendix E.1 — teal #00695C = instrument/corpus data.
                  Single-series chart: identity is carried by the axis title,
                  values by direct labels, so nothing is colour-alone.
*)

scriptDir = DirectoryName[$InputFileName];
repoRoot  = FileNameDrop[scriptDir, -4];
dataFile  = FileNameJoin[{repoRoot, "docs", "design", "stack-safety",
  "measurements", "phase7-depth-histograms-2026-08-03.tsv"}];

teal = RGBColor["#00695C"];
inkPrimary = GrayLevel[0.15]; inkSecondary = GrayLevel[0.38];
gridCol = GrayLevel[0.93];

raw  = Import[dataFile, "TSV"];
rows = Select[raw, Length[#] >= 5 && NumberQ[#[[3]]] && NumberQ[#[[4]]] &];
enc  = Select[rows, #[[1]] === "bincode_encoder" &];
byDepth = KeySort[GroupBy[enc, #[[3]] &, Total[#[[All, 4]]] &]];
depths = Keys[byDepth]; counts = Values[byDepth];
total = Total[counts];
shareLe2 = Total[Select[Normal[byDepth], #[[1]] <= 2 &][[All, 2]]]/total // N;

fmtN[x_] := ToString[NumberForm[Round[x], DigitBlock -> 3, NumberSeparator -> ","]];

lolli = Table[Module[{d = depths[[i]], c = counts[[i]], y},
    y = Log10[c];
    {{teal, AbsoluteThickness[2.2], Line[{{d, 0}, {d, y}}]},
     {teal, PointSize[0.014], Point[{d, y}]},
     Text[Style[fmtN[c], 12, teal, Bold], {d, y + 0.28}, {0, 0}]}],
   {i, Length[depths]}];

fig = Graphics[{
    {gridCol, Thin, Table[Line[{{-0.6, k}, {5.6, k}}], {k, 0, 4}]},
    lolli,
    Text[Style[ToString[NumberForm[100 shareLe2, {4, 1}]] <>
       "% of the " <> fmtN[total] <> " observations lie at depth \[LessEqual] 2:\nproduction terms are shallow \[LongDash] the eliminated depth slopes\ngovern adversarial nesting, not the common case",
      Italic, 11.5, inkSecondary], {3.6, 3.35}, {0, 0}]
  },
  PlotRange -> {{-0.6, 5.6}, {-0.15, 4.6}},
  Frame -> {{True, False}, {True, False}},
  FrameStyle -> Directive[GrayLevel[0.55], AbsoluteThickness[0.8]],
  FrameTicks -> {{Table[{k, fmtN[10^k]}, {k, 0, 4}], None},
    {Table[{d, ToString[d]}, {d, 0, 5}], None}},
  FrameLabel -> {{Style["observations (log scale)", 12.5, inkPrimary], None},
    {Style["nesting depth of the observed root \[LongDash] production cold-store encode corpus (bincode_encoder, five test corpora)",
      12, inkPrimary], None}},
  Background -> White,
  BaseStyle -> {FontFamily -> "DejaVu Sans", FontColor -> inkPrimary},
  ImageSize -> 820, AspectRatio -> 0.55,
  ImagePadding -> {{70, 14}, {40, 10}}];

svgOut = Export[FileNameJoin[{scriptDir, "subject-depth-histogram.svg"}], fig];
Print["written: ", svgOut, " (", FileByteCount[svgOut], " bytes); total ", total,
  " observations; share at depth <= 2 = ", NumberForm[100. shareLe2, {5, 2}], "%; counts ",
  Normal[byDepth]];
