use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let sequence = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "soak-charts-{name}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("test directory should be created");
        Self(path)
    }

    fn path(&self) -> &Path { &self.0 }
}

impl Drop for TestDir {
    fn drop(&mut self) { fs::remove_dir_all(&self.0).ok(); }
}

fn run_renderer(test_dir: &TestDir, history: &str, series: &str) -> Output {
    let history_path = test_dir.path().join("history.json");
    let output_path = test_dir.path().join("output");
    fs::write(&history_path, history).expect("history fixture should be written");

    Command::new(env!("CARGO_BIN_EXE_soak-charts"))
        .args([
            "--history",
            history_path.to_str().expect("history path should be UTF-8"),
            "--out-dir",
            output_path.to_str().expect("output path should be UTF-8"),
            "--series",
            series,
        ])
        .output()
        .expect("renderer should run")
}

fn manifest(test_dir: &TestDir, series: &str) -> Vec<String> {
    let path = test_dir
        .path()
        .join("output")
        .join(format!("charts-manifest-{series}.json"));
    serde_json::from_slice(&fs::read(path).expect("manifest should exist"))
        .expect("manifest should be valid JSON")
}

#[test]
fn zero_failure_cells_are_neutral_and_missing_providers_stay_absent() {
    let test_dir = TestDir::new("zero-and-missing");
    let output = run_renderer(
        &test_dir,
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
        "weekend",
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output_dir = test_dir.path().join("output");
    let light = fs::read_to_string(output_dir.join("failure-heatmap-weekend-light.svg"))
        .expect("light heatmap should exist");
    let dark = fs::read_to_string(output_dir.join("failure-heatmap-weekend-dark.svg"))
        .expect("dark heatmap should exist");

    assert!(light.contains("#e7e5e0"));
    assert!(dark.contains("#33322f"));
    assert!(light.contains("0&#x2F;10"));
    assert!(light.contains("0&#x2F;5"));
    assert!(light.contains(">08-10</text>"));
    assert!(!light.contains(">date</text>"));
    assert!(!light.contains("subprocess"));
    assert_eq!(manifest(&test_dir, "weekend"), vec![
        "failure-heatmap-weekend-light.svg",
        "failure-heatmap-weekend-dark.svg"
    ]);
}

#[test]
fn manifest_tracks_rendered_panels_and_omits_all_zero_alert_panels() {
    let test_dir = TestDir::new("manifest");
    let output = run_renderer(
        &test_dir,
        r#"[
          {
            "run": { "date": "2026-08-10T00:00:00Z" },
            "passive": {
              "iterations": 20,
              "failures": 1,
              "iterations_per_hour": 12.5,
              "too_far_ahead_errors": 0,
              "tracked_metrics": {
                "lfb_spread": { "p95": 2, "max": 4 }
              }
            }
          }
        ]"#,
        "daily",
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let files = manifest(&test_dir, "daily");
    assert!(files.contains(&"failure-heatmap-daily-light.svg".to_string()));
    assert!(files.contains(&"failure-heatmap-daily-dark.svg".to_string()));
    assert!(files.contains(&"chart-throughput-daily-light.svg".to_string()));
    assert!(files.contains(&"chart-throughput-daily-dark.svg".to_string()));
    assert!(files.contains(&"chart-lfb-spread-daily-light.svg".to_string()));
    assert!(files.contains(&"chart-lfb-spread-daily-dark.svg".to_string()));
    let throughput = fs::read_to_string(
        test_dir
            .path()
            .join("output")
            .join("chart-throughput-daily-light.svg"),
    )
    .expect("throughput chart should exist");
    assert!(throughput.contains(">08-10</text>"));
    assert!(!throughput.contains(">date</text>"));
    assert!(!files.iter().any(|file| file.contains("too-far-ahead")));
    assert!(files
        .iter()
        .all(|file| test_dir.path().join("output").join(file).is_file()));
}

#[test]
fn empty_history_writes_an_empty_manifest_without_svg_files() {
    let test_dir = TestDir::new("empty");
    let output = run_renderer(&test_dir, "[]", "weekend");

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(manifest(&test_dir, "weekend").is_empty());

    let svg_count = fs::read_dir(test_dir.path().join("output"))
        .expect("output directory should exist")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "svg")
        })
        .count();
    assert_eq!(svg_count, 0);
}

#[test]
fn unsupported_series_is_rejected_before_output_is_created() {
    let test_dir = TestDir::new("invalid-series");
    let output = run_renderer(&test_dir, "[]", "monthly");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("--series must be weekend or daily, got monthly"));
    assert!(!test_dir.path().join("output").exists());
}

#[test]
fn dense_date_labels_rotate_below_the_x_axis() {
    let test_dir = TestDir::new("dense-dates");
    let output = run_renderer(
        &test_dir,
        r#"[
          { "run": { "date": "2026-08-01T00:00:00Z" }, "passive": { "iterations_per_hour": 10 } },
          { "run": { "date": "2026-08-02T00:00:00Z" }, "passive": { "iterations_per_hour": 11 } },
          { "run": { "date": "2026-08-03T00:00:00Z" }, "passive": { "iterations_per_hour": 12 } },
          { "run": { "date": "2026-08-04T00:00:00Z" }, "passive": { "iterations_per_hour": 13 } }
        ]"#,
        "weekend",
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let light = fs::read_to_string(
        test_dir
            .path()
            .join("output")
            .join("chart-throughput-weekend-light.svg"),
    )
    .expect("dense throughput chart should exist");

    assert!(light.contains("transform=\"rotate(-45"));
    assert!(!light.contains("transform=\"rotate(45"));
}

#[test]
fn per_core_cpu_history_renders_stacked_facets_with_saturation_line() {
    let test_dir = TestDir::new("cpu-facets");
    let output = run_renderer(
        &test_dir,
        r#"[
          {
            "run": { "date": "2026-08-09T00:00:00Z" },
            "passive": {
              "cpu_peak_per_core_pct": { "core-0": 133.0, "core-1": 55.0 }
            }
          },
          {
            "run": { "date": "2026-08-10T00:00:00Z" },
            "passive": {
              "cpu_peak_per_core_pct": { "core-0": 96.0, "core-1": 41.0 }
            }
          }
        ]"#,
        "weekend",
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let light = fs::read_to_string(
        test_dir
            .path()
            .join("output")
            .join("chart-peak-cpu-weekend-light.svg"),
    )
    .expect("light CPU chart should exist");

    // One facet per core, stacked: the second facet is translated down a full
    // canvas, each facet carries its core label, and the 100% saturation line
    // renders in the status-serious color (#c81e1e = rgb(200,30,30)).
    assert!(light.contains("translate(0,400)"));
    assert!(light.contains(">core-0</text>"));
    assert!(light.contains(">core-1</text>"));
    assert!(light.contains("rgba(200,30,30"));
    assert!(light.contains(">08-09</text>"));
    assert!(light.contains(">08-10</text>"));
    assert!(!light.contains(">date</text>"));
    // Clip-path ids must be unique per facet or clipping breaks silently.
    assert!(!light.contains("plot-clip-area"));
    assert!(light.contains("plot-clip-0"));
    assert!(light.contains("plot-clip-1"));
}

#[test]
fn aggregate_only_cpu_history_renders_a_single_unstacked_chart() {
    let test_dir = TestDir::new("cpu-aggregate");
    let output = run_renderer(
        &test_dir,
        r#"[
          {
            "run": { "date": "2026-08-10T00:00:00Z" },
            "passive": { "cpu_peak_pct": 62.8 }
          }
        ]"#,
        "daily",
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let light = fs::read_to_string(
        test_dir
            .path()
            .join("output")
            .join("chart-peak-cpu-daily-light.svg"),
    )
    .expect("light CPU chart should exist");

    assert!(!light.contains("translate(0,400)"));
    assert!(!light.contains(">aggregate</text>"));
}

#[test]
fn cluster_grid_history_renders_a_node_core_heatmap_over_facets() {
    let test_dir = TestDir::new("cpu-grid");
    let output = run_renderer(
        &test_dir,
        r#"[
          {
            "run": { "date": "2026-08-10T00:00:00Z" },
            "passive": {
              "cpu_peak_per_core_pct": { "core-0": 90.0 },
              "cpu_peak_core_grid_pct": {
                "node-1": { "0": 122.0, "1": 85.0 },
                "node-2": { "0": 5.0, "1": 8.0 }
              }
            }
          }
        ]"#,
        "weekend",
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let light = fs::read_to_string(
        test_dir
            .path()
            .join("output")
            .join("chart-peak-cpu-weekend-light.svg"),
    )
    .expect("light CPU chart should exist");

    // The grid wins over the per-core facet path: node ticks on x, no facet
    // stacking, and the one saturated core (122%) carries its printed value.
    assert!(light.contains(">node-1</text>"));
    assert!(light.contains(">node-2</text>"));
    assert!(!light.contains("translate(0,400)"));
    assert!(light.contains(">122</text>"));
}

#[test]
fn cluster_grid_folds_shard_hash_prefixes_into_one_node_column() {
    // Published history carries node keys prefixed per iteration
    // ("f6f7eb46.validator1"): every iteration mints a fresh shard hash, so
    // one run once exploded into 42 columns of unreadable axis labels. The
    // renderer folds keys to their final dot-segment and colliding cells
    // keep the max — the run's true peak — so old entries render readably
    // without republishing.
    let test_dir = TestDir::new("cpu-grid-prefixes");
    let output = run_renderer(
        &test_dir,
        r#"[
          {
            "run": { "date": "2026-08-13T00:00:00Z" },
            "passive": {
              "cpu_peak_core_grid_pct": {
                "f6f7eb46.validator1": { "0": 85.0 },
                "d8b74132.validator1": { "0": 122.0 },
                "d8b74132.boot": { "0": 12.0 }
              }
            }
          }
        ]"#,
        "daily",
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let light = fs::read_to_string(
        test_dir
            .path()
            .join("output")
            .join("chart-peak-cpu-daily-light.svg"),
    )
    .expect("light CPU chart should exist");

    assert!(light.contains(">validator1</text>"));
    assert!(light.contains(">boot</text>"));
    assert!(!light.contains("f6f7eb46"));
    assert!(!light.contains("d8b74132"));
    assert!(light.contains(">122</text>"));
}
