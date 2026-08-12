(* memory-before-after.wl — Figure S4 of the stack-safety technical report.
   Dumbbell plot (log-10 position axis, bytes) of peak-memory wins: the SS-G1
   Arc representation fix (chain_10000 formerly an out-of-memory kill; chain_2000),
   the SS-C12 malformed-input gate resource closure, and the SS-G44 unary
   depth-20,000 parse. The out-of-memory control is drawn hollow and labelled —
   it is a kill, not a completed measurement.

   Data source  : docs/technical-reports/stack-safety/data/memory-wins.tsv
                  (curated from the ledger; provenance machine-checked by
                  tools/mk_tables.py provenance)
   Regenerate   : wolframscript -file memory-before-after.wl   (writes memory-before-after.svg)
   Fallback     : any renderer may substitute matplotlib; keep filenames identical.
   Palette      : ledger Appendix E.1 — slate #455A64 = before, green #2E7D32 =
                  after (CVD-checked pair, deutan dE 12.2); every dot is labelled.
*)

scriptDir = DirectoryName[$InputFileName];
repoRoot  = FileNameDrop[scriptDir, -4];
dataFile  = FileNameJoin[{repoRoot, "docs", "technical-reports", "stack-safety",
  "data", "memory-wins.tsv"}];

slate = RGBColor["#455A64"]; green = RGBColor["#2E7D32"];
inkPrimary = GrayLevel[0.15]; inkSecondary = GrayLevel[0.38];
gridCol = GrayLevel[0.93]; connCol = GrayLevel[0.62];

raw  = Import[dataFile, "TSV"];
rows = Select[raw, Length[#] >= 7 && StringQ[#[[1]]] && ! StringStartsQ[#[[1]], "#"] &&
    ! StringStartsQ[#[[1]], "label"] &];

(* normalize a memory cell — number+unit, possibly with a tilde or a
   parenthetical — into bytes; fail loudly on anything unrecognized *)
parseBytes[v_] := Module[{s, num, unit},
   If[NumberQ[v], Return[N[v] 2^10]];  (* bare numbers in this file are KiB *)
   s = StringReplace[ToString[v], {"~" -> "", RegularExpression["\\s*\\(.*\\)"] -> ""}];
   num = ToExpression[First[StringCases[s, NumberString]]];
   unit = Which[
     StringContainsQ[s, "GiB"], 2.^30, StringContainsQ[s, "MiB"], 2.^20,
     StringContainsQ[s, "KiB"], 2.^10, StringContainsQ[s, "GB"], 1.*^9,
     StringContainsQ[s, "MB"], 1.*^6, StringContainsQ[s, "KB"], 1.*^3,
     True, Message[General::argerr, s]; Abort[]];
   num unit];

isOOM[v_] := StringQ[v] && StringContainsQ[v, "OOM"];

fmtCell[v_] := If[NumberQ[v],
   ToString[NumberForm[Round[v], DigitBlock -> 3, NumberSeparator -> ","]] <> " KiB",
   StringReplace[
    StringReplace[ToString[v], "~" -> "\[TildeTilde]"],
    d : RegularExpression["\\d{4,}"] :>
     ToString[NumberForm[ToExpression[d], DigitBlock -> 3, NumberSeparator -> ","]]]];

n = Length[rows];
yOf[i_] := n - i + 1;

elems = Table[Module[{r = rows[[i]], y, cB, tB, cx, tx},
    y = yOf[i];
    cB = parseBytes[r[[2]]]; tB = parseBytes[r[[3]]];
    cx = Log10[cB]; tx = Log10[tB];
    {{connCol, AbsoluteThickness[1.4], Line[{{tx, y}, {cx, y}}]},
     If[isOOM[r[[2]]],
      {{slate, AbsoluteThickness[1.6], Circle[{cx, y}, Offset[5]]},
       Text[Style[fmtCell[r[[2]]], 10.5, slate, Bold], {cx, y + 0.34}, {0, 0}]},
      {{slate, PointSize[0.011], Point[{cx, y}]},
       Text[Style[fmtCell[r[[2]]], 10.5, slate, Bold], {cx, y + 0.34}, {0, 0}]}],
     {green, PointSize[0.011], Point[{tx, y}]},
     Text[Style[fmtCell[r[[3]]], 10.5, green, Bold], {tx, y + 0.34}, {0, 0}]}],
   {i, n}];

leftTicks = Table[{yOf[i], Style[rows[[i, 1]], 10.5, inkSecondary]}, {i, n}];
tickPow[lo_, hi_] := Table[{k, Superscript[10, k]}, {k, lo, hi}];

fig = Graphics[{
    {gridCol, Thin, Table[Line[{{7, y}, {11.1, y}}], {y, n}]},
    elems,
    Text[Style["hollow ring = out-of-memory kill, not a completed measurement",
      Italic, 11, inkSecondary], {8.4, 4.84}, {0, 0}]
  },
  PlotRange -> {{7, 11.25}, {0.35, n + 1.05}},
  Frame -> {{True, False}, {True, False}},
  FrameStyle -> Directive[GrayLevel[0.55], AbsoluteThickness[0.8]],
  FrameTicks -> {{leftTicks, None}, {tickPow[7, 11], None}},
  FrameLabel -> {{None, None}, {Style["peak memory (bytes, log scale)", 12.5, inkPrimary], None}},
  Background -> White,
  BaseStyle -> {FontFamily -> "DejaVu Sans", FontColor -> inkPrimary},
  ImageSize -> {880, 330}, AspectRatio -> Full,
  ImagePadding -> {{215, 18}, {44, 10}}];

svgOut = Export[FileNameJoin[{scriptDir, "memory-before-after.svg"}], fig];
Print["written: ", svgOut, " (", FileByteCount[svgOut], " bytes); rows ", n];
