(* phase7-ir-exponents.wl — Figure S3 of the stack-safety technical report.
   Cleveland dot plot of the fitted cachegrind instruction-count (Ir) exponents
   for all 40 converted Phase-7 subjects, against the linear reference (1.0)
   and the predeclared quadratic red bar (1.80). No subject is quadratic:
   the observed range is 0.9462 (sort_wide) to 1.0970 (score_cmp_wide).

   Data source  : docs/design/stack-safety/measurements/phase7-cachegrind-fits-2026-08-03.tsv
   Regenerate   : wolframscript -file phase7-ir-exponents.wl   (writes phase7-ir-exponents.svg)
   Fallback     : any renderer may substitute matplotlib; keep filenames identical.
   Palette      : ledger Appendix E.1 — green #2E7D32 = converted subjects,
                  slate #455A64 = the linear reference, amber #EF6C00 = the
                  predeclared threshold. Threshold and reference are DASHED and
                  text-labelled (annotations, not series), so nothing is
                  colour-alone; width-axis subjects are named "(width)" in text.
*)

scriptDir = DirectoryName[$InputFileName];
repoRoot  = FileNameDrop[scriptDir, -4];
dataFile  = FileNameJoin[{repoRoot, "docs", "design", "stack-safety",
  "measurements", "phase7-cachegrind-fits-2026-08-03.tsv"}];

slate = RGBColor["#455A64"]; green = RGBColor["#2E7D32"]; amber = RGBColor["#EF6C00"];
inkPrimary = GrayLevel[0.15]; inkSecondary = GrayLevel[0.38];
gridCol = GrayLevel[0.93];

raw  = Import[dataFile, "TSV"];
rows = Select[raw, Length[#] >= 7 && NumberQ[#[[4]]] &];  (* drop the header *)
(* row: axis, subject, points, Ir, Dr, Dw, verdict *)
sorted = SortBy[rows, #[[4]] &];
n = Length[sorted];

label[row_] := row[[2]] <> If[row[[1]] === "width", "  (width)", ""];
leftTicks = Table[{i, Style[label[sorted[[i]]], 9.5, inkSecondary]}, {i, n}];

(* selective direct value labels: the two extremes and the two PathMap hashing
   subjects the PathMap technical report cites *)
called = {"sort_wide", "score_cmp_wide", "hash_pathmap_set", "hash_pathmap_map"};
valueLabels = Table[
   If[MemberQ[called, sorted[[i, 2]]],
    Text[Style[ToString[NumberForm[sorted[[i, 4]], {5, 4}]], 10, green, Bold],
     {sorted[[i, 4]] + 0.012, i}, {-1, 0}],
    Nothing], {i, n}];

fig = Graphics[{
    {gridCol, Thin, Table[Line[{{0.90, i}, {1.90, i}}], {i, n}]},
    (* linear reference at 1.0 *)
    {slate, Dashed, AbsoluteThickness[1.2], Line[{{1.0, 0}, {1.0, n + 1.2}}]},
    Text[Style["linear: exponent 1.0", 11, slate, Bold], {1.0, n + 2.2}, {0, 0}],
    (* predeclared quadratic red bar *)
    {amber, Dashed, AbsoluteThickness[1.6], Line[{{1.80, 0}, {1.80, n + 1.2}}]},
    Text[Style["predeclared quadratic\nred bar: 1.80", 11, amber, Bold], {1.80, n + 2.6}, {0, 0}],
    (* the subjects *)
    {green, PointSize[0.0085], Table[Point[{sorted[[i, 4]], i}], {i, n}]},
    valueLabels,
    Text[Style["all 40 converted subjects fit subquadratic:\nIr exponent range 0.9462 \[Dash] 1.0970",
      Italic, 11.5, inkSecondary], {1.46, 12}, {0, 0}]
  },
  PlotRange -> {{0.90, 1.90}, {0, n + 4.4}},
  Frame -> {{True, False}, {True, False}},
  FrameStyle -> Directive[GrayLevel[0.55], AbsoluteThickness[0.8]],
  FrameTicks -> {{leftTicks, None},
    {Table[{x, ToString[NumberForm[x, {3, 1}]]}, {x, 0.9, 1.9, 0.1}], None}},
  FrameLabel -> {{None, None},
    {Style["fitted cachegrind Ir exponent over depth rungs 128 / 256 / 512 / 768", 12.5, inkPrimary], None}},
  Background -> White,
  BaseStyle -> {FontFamily -> "DejaVu Sans", FontColor -> inkPrimary},
  ImageSize -> {880, 860}, AspectRatio -> Full,
  ImagePadding -> {{178, 14}, {44, 10}}];

svgOut = Export[FileNameJoin[{scriptDir, "phase7-ir-exponents.svg"}], fig];
Print["written: ", svgOut, " (", FileByteCount[svgOut], " bytes); subjects ", n,
  "; Ir range ", NumberForm[Min[rows[[All, 4]]], {5, 4}], " to ",
  NumberForm[Max[rows[[All, 4]]], {5, 4}]];
