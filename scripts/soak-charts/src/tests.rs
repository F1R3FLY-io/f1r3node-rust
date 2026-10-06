use super::*;

fn history(json: &str) -> Vec<Entry> {
    serde_json::from_str(json).expect("history fixture should deserialize")
}

#[test]
fn parse_date_accepts_iso_dates_and_rejects_invalid_calendar_values() {
    let (_, label) = parse_date("2026-08-10T12:34:56Z").expect("valid date should parse");

    assert_eq!(label, "08-10");
    assert!(parse_date("2026/08/10").is_none());
    assert!(parse_date("2026-02-30T00:00:00Z").is_none());
}

#[test]
fn heat_cells_distinguish_zero_failures_from_missing_providers() {
    let entries = history(
        r#"[
          {
            "run": { "date": "2026-08-10T00:00:00Z" },
            "passive": {
              "iterations": 10,
              "failures": 0,
              "providers": {
                "docker": { "iterations": 5, "failures": 0 }
              }
            }
          }
        ]"#,
    );

    let cells = collect_heat_cells(&entries);

    assert_eq!(cells.len(), 2);
    assert!(cells
        .iter()
        .any(|cell| cell.category == "total" && cell.rate == 0.0));
    assert!(cells
        .iter()
        .any(|cell| cell.category == "docker" && cell.iterations == 5.0));
    assert!(!cells.iter().any(|cell| cell.category == "subprocess"));
}

#[test]
fn panel_collection_ignores_invalid_dates_and_non_finite_values() {
    let entries = history(
        r#"[
          {
            "run": { "date": "2026-08-10T00:00:00Z" },
            "passive": { "iterations_per_hour": 12.5 }
          },
          {
            "run": { "date": "invalid" },
            "passive": { "iterations_per_hour": 99 }
          }
        ]"#,
    );

    let observations = collect_panel(&entries, &PANELS[0]);

    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].series, "total");
    assert_eq!(observations[0].value, 12.5);
    assert_eq!(observations[0].label, "08-10");
}

#[test]
fn sparse_value_formatting_preserves_useful_precision() {
    assert_eq!(fmt_value(120.4), "120");
    assert_eq!(fmt_value(12.0), "12");
    assert_eq!(fmt_value(0.125), "0.12");
}

#[test]
fn cpu_rows_prefer_per_core_readings_and_synthesize_aggregate_otherwise() {
    let entries = history(
        r#"[
          {
            "run": { "date": "2026-08-09T00:00:00Z" },
            "passive": { "cpu_peak_pct": 62.8 }
          },
          {
            "run": { "date": "2026-08-10T00:00:00Z" },
            "passive": {
              "cpu_peak_pct": 133.0,
              "cpu_peak_per_core_pct": { "core-0": 133.0, "core-1": 55.5 }
            }
          }
        ]"#,
    );

    let rows = collect_cpu(&entries);

    assert_eq!(rows.len(), 3);
    assert!(rows
        .iter()
        .any(|row| row.core == "aggregate" && row.label == "08-09" && row.pct == 62.8));
    assert!(rows
        .iter()
        .any(|row| row.core == "core-0" && row.label == "08-10" && row.pct == 133.0));
    assert!(rows
        .iter()
        .any(|row| row.core == "core-1" && row.label == "08-10" && row.pct == 55.5));
    assert!(
        !rows
            .iter()
            .any(|row| row.core == "aggregate" && row.label == "08-10"),
        "per-core readings must replace the synthesized aggregate for their run"
    );
}

#[test]
fn xml_escape_neutralizes_markup_in_core_ids() {
    assert_eq!(
        esc_xml(r#"<core "0" & 'more'>"#),
        "&lt;core &quot;0&quot; &amp; &#39;more&#39;&gt;"
    );
}

#[test]
fn latest_cpu_grid_picks_the_most_recent_run_and_ignores_empty_grids() {
    let entries = history(
        r#"[
          {
            "run": { "date": "2026-08-09T00:00:00Z" },
            "passive": { "cpu_peak_core_grid_pct": { "node-1": { "0": 40.0 } } }
          },
          {
            "run": { "date": "2026-08-10T00:00:00Z" },
            "passive": { "cpu_peak_core_grid_pct": { "node-1": { "0": 75.0 } } }
          },
          {
            "run": { "date": "2026-08-11T00:00:00Z" },
            "passive": { "cpu_peak_core_grid_pct": { "node-1": {} } }
          }
        ]"#,
    );

    let grid = latest_cpu_grid(&entries).expect("a grid should be found");

    assert_eq!(
        grid["node-1"]["0"], 75.0,
        "empty grids must not shadow the last real one"
    );
}

#[test]
fn id_sorting_is_numeric_for_numeric_ids_and_lexicographic_otherwise() {
    let numeric = id_sorted(["10", "2", "0", "1"].map(String::from).into_iter());
    assert_eq!(numeric, ["0", "1", "2", "10"]);

    let named = id_sorted(["node-b", "node-a"].map(String::from).into_iter());
    assert_eq!(named, ["node-a", "node-b"]);
}
