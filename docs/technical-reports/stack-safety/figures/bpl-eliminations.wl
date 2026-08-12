(* bpl-eliminations.wl — Figure S1 of the stack-safety technical report.
   Cleveland dot plot (log-10 position axis) of the pre-conversion native-stack
   slopes (B/level) of the headline converted traversals; after conversion every
   subject measures 0 B/level (flat at both ends of a 4 -> 4,096 depth ladder in
   both build profiles), so the zero is set as typography ("-> 0") beside each
   dot — a logarithmic axis cannot plot 0, and dot POSITION (not bar length)
   carries the value.

   Data source  : docs/technical-reports/stack-safety/data/bpl-conversions.tsv
                  (curated from the ledger's Abstract headline table; provenance
                  machine-checked by tools/mk_tables.py provenance)
   Regenerate   : wolframscript -file bpl-eliminations.wl   (writes bpl-eliminations.svg)
   Fallback     : any renderer may substitute matplotlib; keep filenames identical.
   Palette      : ledger Appendix E.1 — slate #455A64 = pre-conversion (control),
                  green #2E7D32 = converted/after. Pair passes the CVD check
                  (deutan dE 12.2); every dot carries a direct text label.
*)

scriptDir = DirectoryName[$InputFileName];
repoRoot  = FileNameDrop[scriptDir, -4];
dataFile  = FileNameJoin[{repoRoot, "docs", "technical-reports", "stack-safety",
  "data", "bpl-conversions.tsv"}];

slate = RGBColor["#455A64"]; green = RGBColor["#2E7D32"];
inkPrimary = GrayLevel[0.15]; inkSecondary = GrayLevel[0.38];
gridCol = GrayLevel[0.93];

raw  = Import[dataFile, "TSV"];
rows = Select[raw, Length[#] >= 7 && StringQ[#[[1]]] && ! StringStartsQ[#[[1]], "#"] &&
    NumberQ[#[[2]]] &];
sorted = SortBy[rows, #[[2]] &];  (* ascending; largest slope ends on top *)
n = Length[sorted];

fmtN[x_] := ToString[NumberForm[Round[x], DigitBlock -> 3, NumberSeparator -> ","]];
rowLabel[r_] := r[[1]] <> "  (" <> r[[5]] <> ")";
leftTicks = Table[{i, Style[rowLabel[sorted[[i]]], 10.5, inkSecondary]}, {i, n}];
tickPow[lo_, hi_] := Table[{k, Superscript[10, k]}, {k, lo, hi}];

dots = Table[Module[{v = sorted[[i, 2]], x},
    x = Log10[v];
    {{slate, PointSize[0.010], Point[{x, i}]},
     Text[Style[fmtN[v] <> " B/level", 10.5, slate, Bold], {x, i + 0.42}, {0, 0}],
     Text[Style["\[RightArrow] 0", 11.5, green, Bold], {x + 0.22, i}, {-1, 0}]}],
   {i, n}];

fig = Graphics[{
    {gridCol, Thin, Table[Line[{{1.6, i}, {5.6, i}}], {i, n}]},
    dots,
    Text[Style["every subject re-measured flat after conversion:\n0 B/level at both ends of a 4 \[RightArrow] 4,096 ladder, both profiles\n(\"\[RightArrow] 0\" is stated, not plotted \[LongDash] a log axis has no zero)",
      Italic, 11.5, inkSecondary], {4.88, 2.05}, {0, 0}]
  },
  PlotRange -> {{1.6, 6.05}, {0.2, n + 0.9}},
  Frame -> {{True, False}, {True, False}},
  FrameStyle -> Directive[GrayLevel[0.55], AbsoluteThickness[0.8]],
  FrameTicks -> {{leftTicks, None}, {tickPow[2, 5], None}},
  FrameLabel -> {{None, None},
    {Style["pre-conversion native-stack slope (B/level, log scale)", 12.5, inkPrimary], None}},
  Background -> White,
  BaseStyle -> {FontFamily -> "DejaVu Sans", FontColor -> inkPrimary},
  ImageSize -> {920, 470}, AspectRatio -> Full,
  ImagePadding -> {{288, 16}, {44, 10}}];

svgOut = Export[FileNameJoin[{scriptDir, "bpl-eliminations.svg"}], fig];
Print["written: ", svgOut, " (", FileByteCount[svgOut], " bytes); rows ", n,
  "; slopes ", fmtN[Min[rows[[All, 2]]]], " to ", fmtN[Max[rows[[All, 2]]]], " B/level"];
