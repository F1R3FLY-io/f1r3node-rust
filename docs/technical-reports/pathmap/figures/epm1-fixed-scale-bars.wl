(* epm1-fixed-scale-bars.wl — Figure P2 of the PathMap technical report.
   Two dumbbell panels at the fixed 1,024-entry scale (log-10 position axes):
   (top) serialized bytes — EPM1 trie snapshot vs the flat list-of-Par
   projection it replaced; (bottom) lookup latency per key — native indexed
   lookup vs a linear scan over the projection. Position on a log axis is the
   encoding (dumbbell dots), never bar length, and every dot carries its value.

   Data source  : docs/design/pathmap/measurements/epm1-fixed-scale-2026-08-03.tsv
   Regenerate   : wolframscript -file epm1-fixed-scale-bars.wl   (writes epm1-fixed-scale-bars.svg)
   Fallback     : any renderer may substitute matplotlib; keep filenames identical.
   Palette      : ledger Appendix E.1 — slate #455A64 = control (list projection),
                  green #2E7D32 = treatment (EPM1 / native). The pair passes the
                  colour-vision-deficiency check (deutan dE 12.2) and every dot is
                  direct-labelled, so identity is never colour-alone.
*)

scriptDir = DirectoryName[$InputFileName];
repoRoot  = FileNameDrop[scriptDir, -4];
dataFile  = FileNameJoin[{repoRoot, "docs", "design", "pathmap",
  "measurements", "epm1-fixed-scale-2026-08-03.tsv"}];

slate = RGBColor["#455A64"]; green = RGBColor["#2E7D32"];
inkPrimary = GrayLevel[0.15]; inkSecondary = GrayLevel[0.38];
gridCol = GrayLevel[0.93]; connCol = GrayLevel[0.62];

raw  = Import[dataFile, "TSV"];
rows = Select[raw, Length[#] >= 4 && StringQ[#[[1]]] && ! StringStartsQ[#[[1]], "#"] &];
val[measurement_, col_] := Module[{rowFound = SelectFirst[rows, #[[1]] === measurement &]},
  rowFound[[col]]];  (* col 3 = set, col 4 = map *)

fmtN[x_] := ToString[NumberForm[Round[x], DigitBlock -> 3, NumberSeparator -> ","]];
fmtD[x_] := ToString[NumberForm[x, {6, 2}]];

(* ---------- panel builder: one dumbbell row per subject ---------- *)
tickPow[lo_, hi_] := Table[{k, Superscript[10, k]}, {k, lo, hi}];

dumbbell[y_, cVal_, tVal_, cLab_, tLab_, ratioLab_] := {
   {connCol, AbsoluteThickness[1.4], Line[{{Log10[tVal], y}, {Log10[cVal], y}}]},
   {green, PointSize[0.012], Point[{Log10[tVal], y}]},
   {slate, PointSize[0.012], Point[{Log10[cVal], y}]},
   Text[Style[tLab, 11, green, Bold], {Log10[tVal], y + 0.26}, {0, 0}],
   Text[Style[cLab, 11, slate, Bold], {Log10[cVal], y + 0.26}, {0, 0}],
   Text[Style[ratioLab, 11.5, inkPrimary, Bold],
    {(Log10[tVal] + Log10[cVal])/2, y - 0.30}, {0, 0}]};

panel[title_, rowsSpec_, xlo_, xhi_, xlabel_] := Graphics[{
    {gridCol, Thin, Table[Line[{{k, 0.45}, {k, 2.9}}], {k, xlo, xhi}]},
    Text[Style[title, 13, inkPrimary, Bold], {(xlo + xhi)/2, 3.28}, {0, 0}],
    rowsSpec
  },
  PlotRange -> {{xlo - 0.15, xhi + 0.15}, {0.3, 3.6}},
  Frame -> {{True, False}, {True, False}},
  FrameStyle -> Directive[GrayLevel[0.55], AbsoluteThickness[0.8]],
  FrameTicks -> {{{{2, Style["set", 12, inkPrimary]}, {1, Style["map", 12, inkPrimary]}}, None},
    {tickPow[xlo, xhi], None}},
  FrameLabel -> {{None, None}, {Style[xlabel, 12, inkPrimary], None}},
  Background -> White,
  BaseStyle -> {FontFamily -> "DejaVu Sans", FontColor -> inkPrimary},
  ImageSize -> 860, AspectRatio -> 0.30,
  ImagePadding -> {{48, 14}, {40, 8}}];

bytesPanel = panel[
   "serialized bytes at 1,024 entries \[LongDash] EPM1 (green) vs list projection (slate)",
   {dumbbell[2, val["list_projection_bytes", 3], val["epm1_bytes", 3],
     fmtN[val["list_projection_bytes", 3]] <> " B", fmtN[val["epm1_bytes", 3]] <> " B",
     fmtD[val["size_ratio_projection_over_epm1", 3]] <> "\[Times] smaller"],
    dumbbell[1, val["list_projection_bytes", 4], val["epm1_bytes", 4],
     fmtN[val["list_projection_bytes", 4]] <> " B", fmtN[val["epm1_bytes", 4]] <> " B",
     fmtD[val["size_ratio_projection_over_epm1", 4]] <> "\[Times] smaller"]},
   3, 6, "serialized size (B, log scale)"];

lookupPanel = panel[
   "lookup latency per key \[LongDash] native indexed (green) vs linear scan over the projection (slate)",
   {dumbbell[2, val["linear_projected_lookup", 3], val["native_indexed_lookup", 3],
     fmtN[val["linear_projected_lookup", 3]] <> " ns", fmtD[val["native_indexed_lookup", 3]] <> " ns",
     fmtN[val["lookup_ratio_projected_over_native", 3]] <> "\[Times] faster"],
    dumbbell[1, val["linear_projected_lookup", 4], val["native_indexed_lookup", 4],
     fmtN[val["linear_projected_lookup", 4]] <> " ns", fmtD[val["native_indexed_lookup", 4]] <> " ns",
     fmtN[val["lookup_ratio_projected_over_native", 4]] <> "\[Times] faster"]},
   1, 6, "lookup latency (ns per key, log scale)"];

fig = GraphicsColumn[{bytesPanel, lookupPanel}, Spacings -> 6, ImageSize -> 880,
  Background -> White];

svgOut = Export[FileNameJoin[{scriptDir, "epm1-fixed-scale-bars.svg"}], fig];
Print["written: ", svgOut, " (", FileByteCount[svgOut], " bytes); set ratio ",
  fmtD[val["size_ratio_projection_over_epm1", 3]], "x, map ratio ",
  fmtD[val["size_ratio_projection_over_epm1", 4]], "x, lookup ratios ",
  fmtN[val["lookup_ratio_projected_over_native", 3]], "x / ",
  fmtN[val["lookup_ratio_projected_over_native", 4]], "x"];
