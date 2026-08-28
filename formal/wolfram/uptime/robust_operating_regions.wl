reportPath = Environment["UPTIME_STORM_REPORT"];
If[reportPath === $Failed || reportPath === "",
  reportPath = "target/verification/uptime/storm/engineering-envelope.json"
];

report = Import[reportPath, "RawJSON"];
scenarios = report["scenarios"];
lifetimes = scenarios[[All, "expected_lifetime_hours"]];
survivals = scenarios[[All, "month_uninterrupted_survival_probability"]];
downHours = scenarios[[2, "expected_month_down_hours"]];
horizon = report["horizon_hours"];

assert[name_, condition_] := If[TrueQ[condition], Null,
  Print["[robust_operating_regions] FAIL: " <> name]; Exit[1]
];

assert["three lifetime scenarios", Length[lifetimes] == 3];
assert["three survival scenarios", Length[survivals] == 3];
assert["ordered lifetime envelope", OrderedQ[lifetimes]];
assert["ordered survival envelope", OrderedQ[survivals]];
assert["probability domain", And @@ Thread[0 <= survivals <= 1]];
assert["positive horizon", horizon > 0];
assert["bounded nominal downtime", 0 <= downHours <= horizon];

robustLifetime = Minimize[{lifetime, Or @@ Thread[lifetime == lifetimes]}, lifetime][[1]];
robustSurvival = Minimize[{survival, Or @@ Thread[survival == survivals]}, survival][[1]];
nominalAvailability = 1 - downHours/horizon;
feasibleRegion = FullSimplify[Reduce[
  0 <= lifetimeFloor <= robustLifetime &&
  0 <= survivalFloor <= robustSurvival &&
  0 <= availabilityFloor <= nominalAvailability,
  {lifetimeFloor, survivalFloor, availabilityFloor}, Reals
]];

assert["robust lifetime", robustLifetime == Min[lifetimes]];
assert["robust survival", robustSurvival == Min[survivals]];
assert["nonempty operating region", feasibleRegion =!= False];
assert["engineering envelope cannot certify",
  report["evidence_class"] === "engineering_envelope" &&
  report["release_certified"] === False
];

Print["[robust_operating_regions] robust expected lifetime hours: ", N[robustLifetime, 12]];
Print["[robust_operating_regions] robust 30-day survival: ", N[robustSurvival, 12]];
Print["[robust_operating_regions] nominal interval availability: ", N[nominalAvailability, 12]];
Print["[robust_operating_regions] SELF-TEST: PASS"];
