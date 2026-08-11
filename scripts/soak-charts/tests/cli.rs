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
              "too_far_ahead_errors": 0
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
