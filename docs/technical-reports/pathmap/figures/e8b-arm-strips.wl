(* e8b-arm-strips.wl — Figure P1 of the PathMap technical report.
   Jittered strip plot of the 51 measured injection times per arm of the
   preregistered E-8b cell (pgmcp experiment 171): control (spread-and-drive)
   versus treatment (native PathMap<Par> evaluation), with per-arm mean bars.
   Every treatment sample lies below every control sample (Cliff's delta = -1).

   Data source  : docs/design/stack-safety/measurements/e8b-native-pathmap-par-e6a-2026-08-04.tsv
   Regenerate   : wolframscript -file e8b-arm-strips.wl   (writes e8b-arm-strips.svg beside this file)
   Fallback     : any renderer may substitute matplotlib; keep filenames identical.
   Palette      : ledger Appendix E.1 concept colours — slate #455A64 = control/before,
                  green #2E7D32 = converted/after. The slate/green pair passes the
                  colour-vision-deficiency check (deutan dE 12.2); each strip also
                  carries a direct text label, so identity is never colour-alone.
*)

scriptDir = DirectoryName[$InputFileName];
repoRoot  = FileNameDrop[scriptDir, -4];
dataFile  = FileNameJoin[{repoRoot, "docs", "design", "stack-safety",
  "measurements", "e8b-native-pathmap-par-e6a-2026-08-04.tsv"}];

slate = RGBColor["#455A64"]; green = RGBColor["#2E7D32"];
inkPrimary = GrayLevel[0.15]; inkSecondary = GrayLevel[0.38];
gridCol = GrayLevel[0.92];

raw  = Import[dataFile, "TSV"];
rows = Select[raw, Length[#] >= 4 && NumberQ[#[[3]]] && NumberQ[#[[4]]] &];
control   = rows[[All, 3]];
treatment = rows[[All, 4]];
nArm = Length[control];

meanC = Mean[control // N]; meanT = Mean[treatment // N];
fmtMs[x_] := ToString[NumberForm[x, {7, 3}]] <> " ms";

SeedRandom[20260812];
controlPts   = Thread[{1 + RandomReal[{-0.11, 0.11}, nArm], control}];
treatmentPts = Thread[{2 + RandomReal[{-0.11, 0.11}, nArm], treatment}];

meanBar[xc_, m_, col_] := {col, Thickness[0.004], Line[{{xc - 0.22, m}, {xc + 0.22, m}}]};

fig = Graphics[{
    (* horizontal reference grid *)
    {gridCol, Thin, Table[Line[{{0.45, y}, {2.55, y}}], {y, 18, 33, 3}]},
    (* sample points *)
    {slate, PointSize[0.0065], Point[controlPts]},
    {green, PointSize[0.0065], Point[treatmentPts]},
    (* per-arm mean bars and labels *)
    meanBar[1, meanC, slate],
    meanBar[2, meanT, green],
    Text[Style["mean " <> fmtMs[meanC], 12, slate, Bold], {1, meanC + 0.62}, {0, 0}],
    Text[Style["mean " <> fmtMs[meanT], 12, green, Bold], {2, meanT + 0.62}, {0, 0}],
    (* arm labels (direct identity, not colour-alone) *)
    Text[Style["control\nspread-and-drive", 12, slate, Bold], {1, 34.4}, {0, 0}],
    Text[Style["treatment\nnative PathMap\[LeftAngleBracket]Par\[RightAngleBracket]", 12, green, Bold], {2, 34.4}, {0, 0}],
    (* the separation annotation *)
    Text[Style["complete separation: every treatment sample is below\nevery control sample (Cliff's \[Delta] = \[Minus]1; n = 51 per arm)",
      Italic, 11.5, inkSecondary], {1.5, 25.6}, {0, 0}]
  },
  PlotRange -> {{0.45, 2.55}, {17.5, 35.6}},
  Frame -> {{True, False}, {False, False}},
  FrameStyle -> Directive[GrayLevel[0.55], AbsoluteThickness[0.8]],
  FrameTicks -> {{Table[{y, ToString[y]}, {y, 18, 33, 3}], None}, {None, None}},
  FrameLabel -> {{Style["injection wall-clock per swap_comb run, n = 16 (ms)", 12.5, inkPrimary], None}, {None, None}},
  Background -> White,
  BaseStyle -> {FontFamily -> "DejaVu Sans", FontColor -> inkPrimary},
  ImageSize -> 780, AspectRatio -> 0.75, PlotRangePadding -> None,
  ImagePadding -> {{62, 12}, {14, 8}}];

svgOut = Export[FileNameJoin[{scriptDir, "e8b-arm-strips.svg"}], fig];
Print["written: ", svgOut, " (", FileByteCount[svgOut], " bytes); mean control ",
  fmtMs[meanC], ", mean treatment ", fmtMs[meanT],
  ", max treatment ", fmtMs[Max[treatment]], " < min control ", fmtMs[Min[control]]];
