//! Publish-time chart renderer for the soak dashboard.
//!
//! Reads a soak history series (history.json / history-daily.json) and emits
//! static SVG charts into the Pages site tree, one light and one dark variant
//! per chart — colors are baked at render time, and the page swaps variants
//! with the same theme logic as the rest of the dashboard. Run by the
//! publishers whose output can change history — the final soak publish and
//! dashboard edits; checkpoint publishes carry the previous SVGs forward
//! unchanged, because a checkpoint never appends to history.
//!
//! Alongside the SVGs it writes `charts-manifest-<series>.json`, the list of
//! files it produced. The publishers' carry-forward loops iterate that
//! manifest instead of hardcoding chart filenames, so adding a chart here is
//! the whole job — no publisher edit, no three-list hazard.
//!
//! Two chart families:
//!
//! Failure heatmap — rows are failure categories (total plus per provider),
//! columns are run dates, cell color is the failure rate on a sequential red
//! ramp with a neutral non-red for 0% (so "ran clean" can never be misread as
//! "barely failed"), and cell text carries volume ("3/176"). Dates a category
//! did not run leave no cell, keeping absence and zero visually distinct.
//!
//! Metric panels — the time-series charts (throughput, peak RSS/CPU,
//! finalization latency, too-far-ahead, LFB spread). Mark choice is decided by
//! data density, per panel and per render: with more than SPARSE_MAX_DATES
//! distinct dates a panel is a layered line+point chart on a temporal axis
//! (throughput adds a low-opacity area to emphasize its trend); at or below it
//! a line would be an empty-looking fraud, so the panel renders as value-
//! labelled bars (or labelled points when several series share several dates).
//! A panel with no data emits nothing, and the all-zero too-far-ahead panel is
//! suppressed outright — the page shows a "0" badge instead of a flat line.

use std::error::Error;
use std::process::ExitCode;
use std::{env, fs};

use charton::prelude::*;
use charton::scale::{Expansion, ScaleDomain};
use serde::Deserialize;

#[derive(Deserialize)]
struct Entry {
    run: Option<Run>,
    passive: Option<Passive>,
    active: Option<Active>,
}

#[derive(Deserialize)]
struct Run {
    date: Option<String>,
}

#[derive(Deserialize)]
struct Passive {
    iterations: Option<f64>,
    failures: Option<f64>,
    providers: Option<Providers>,
    iterations_per_hour: Option<f64>,
    rss_peak_mb: Option<f64>,
    cpu_peak_pct: Option<f64>,
    /// Per-core peak utilization (core id -> % of that core), not emitted by
    /// the soak yet. The CPU panel is shaped around this future matrix: when
    /// rows appear they become facets with no renderer change, and until then
    /// a single synthesized "aggregate" facet carries cpu_peak_pct.
    cpu_peak_per_core_pct: Option<std::collections::BTreeMap<String, f64>>,
    finalization_p50_ms: Option<f64>,
    finalization_p95_ms: Option<f64>,
    finalization_p99_ms: Option<f64>,
    too_far_ahead_errors: Option<f64>,
    tracked_metrics: Option<TrackedMetrics>,
}

#[derive(Deserialize)]
struct TrackedMetrics {
    lfb_spread: Option<LfbSpread>,
}

#[derive(Deserialize)]
struct LfbSpread {
    p95: Option<f64>,
    max: Option<f64>,
}

#[derive(Deserialize)]
struct Active {
    rss_peak_mb: Option<f64>,
    p95_ms: Option<f64>,
}

#[derive(Deserialize)]
struct Providers {
    docker: Option<ProviderStats>,
    subprocess: Option<ProviderStats>,
}

#[derive(Deserialize)]
struct ProviderStats {
    iterations: Option<f64>,
    failures: Option<f64>,
}

struct Theme {
    name: &'static str,
    /// Sequential ramp for nonzero failure rates (light -> dark red).
    ramp: ColorMap,
    /// The exact fill charton emits for a rate normalizing to 0.0 on `ramp` —
    /// the substitution target in retheme_svg. ColorBrewer Reds starts at
    /// rgb(255,245,240).
    ramp_start: &'static str,
    /// Neutral replacement for 0% cells: visibly "ran clean", never red, and
    /// distinct from the page surface so absence (no cell) still reads apart.
    zero_neutral: &'static str,
    /// Whether zero_neutral is a dark fill (true on the dark theme, where the
    /// neutral is near-surface dark while low-rate ramp cells stay light).
    zero_is_dark: bool,
    /// Ink for text sitting on light fills.
    ink_on_light: &'static str,
    /// Ink for text sitting on dark fills.
    ink_on_dark: &'static str,
    /// Canvas background baked into the SVG — the dashboard figure's surface
    /// color (--surface-2), so the chart sits on the same ground as the
    /// page-drawn elements instead of a white box. Distinct from zero_neutral,
    /// so absence (bare surface) and 0% (neutral cell) stay tellable apart.
    surface: &'static str,
    /// Ink for axis titles and tick labels (--text-secondary). Axis LINES have
    /// no theme hook in charton and keep its default rgba(51,51,51) stroke;
    /// retheme_svg rewrites that stroke to this ink.
    axis_ink: &'static str,
    /// The dashboard's fixed series palette, matching --series-1/2/3 for this
    /// theme (total/p50 = blue, docker/p95 = green, subprocess/bench/p99 =
    /// pink). Color follows the entity, never its rank: a panel's series keep
    /// their slot even when siblings are absent.
    series: [&'static str; 3],
    /// Status color for "serious" (--serious): reserved for state, never a
    /// data series. Used by the CPU panel's 100% saturation line.
    serious: &'static str,
}

const THEMES: [Theme; 2] = [
    Theme {
        name: "light",
        ramp: ColorMap::Reds,
        ramp_start: "rgba(255,245,240,1.000)",
        zero_neutral: "#e7e5e0",
        zero_is_dark: false,
        ink_on_light: "#52514e",
        ink_on_dark: "#ffffff",
        surface: "#f1f0ee",
        axis_ink: "#52514e",
        series: ["#2a78d6", "#008300", "#d5588e"],
        serious: "#c81e1e",
    },
    Theme {
        name: "dark",
        ramp: ColorMap::Reds,
        ramp_start: "rgba(255,245,240,1.000)",
        zero_neutral: "#33322f",
        zero_is_dark: true,
        ink_on_light: "#1a1a19",
        ink_on_dark: "#e8e7df",
        surface: "#242423",
        axis_ink: "#c3c2b7",
        series: ["#3987e5", "#008300", "#d55181"],
        serious: "#f05252",
    },
];

/// At or below this many distinct dates a line chart misrepresents the data —
/// mostly empty axis with a stranded segment — so panels switch to bars or
/// labelled points. The line comes back on its own once density improves.
const SPARSE_MAX_DATES: usize = 3;

struct SeriesDef {
    name: &'static str,
    /// Slot in Theme::series — the same index the page's legend uses.
    color: usize,
    get: fn(&Entry) -> Option<f64>,
}

struct PanelDef {
    slug: &'static str,
    /// Y column name — charton titles the axis with it, so it carries the
    /// unit (and for CPU the multi-core clarification).
    y_title: &'static str,
    series: &'static [SeriesDef],
    /// Low-opacity area under the first series (trend emphasis; throughput).
    area_under_first: bool,
    /// Suppress the chart entirely while every value is zero — the page
    /// renders a "0" badge instead of a flat line (too-far-ahead).
    skip_if_all_zero: bool,
}

fn p(e: &Entry) -> Option<&Passive> { e.passive.as_ref() }
fn a(e: &Entry) -> Option<&Active> { e.active.as_ref() }
fn lfb(e: &Entry) -> Option<&LfbSpread> { p(e)?.tracked_metrics.as_ref()?.lfb_spread.as_ref() }

const PANELS: [PanelDef; 7] = [
    PanelDef {
        slug: "throughput",
        y_title: "iterations / hour",
        series: &[SeriesDef {
            name: "total",
            color: 0,
            get: |e| p(e)?.iterations_per_hour,
        }],
        area_under_first: true,
        skip_if_all_zero: false,
    },
    PanelDef {
        slug: "peak-rss",
        y_title: "resident set size, MB",
        series: &[
            SeriesDef {
                name: "soak load",
                color: 0,
                get: |e| p(e)?.rss_peak_mb,
            },
            SeriesDef {
                name: "bench segments",
                color: 2,
                get: |e| a(e)?.rss_peak_mb,
            },
        ],
        area_under_first: false,
        skip_if_all_zero: false,
    },
    PanelDef {
        slug: "peak-cpu",
        y_title: "% of one core",
        series: &[SeriesDef {
            name: "soak load",
            color: 0,
            get: |e| p(e)?.cpu_peak_pct,
        }],
        area_under_first: false,
        skip_if_all_zero: false,
    },
    PanelDef {
        slug: "finality-p95",
        y_title: "ms",
        series: &[
            SeriesDef {
                name: "soak load",
                color: 0,
                get: |e| p(e)?.finalization_p95_ms,
            },
            SeriesDef {
                name: "bench segments",
                color: 2,
                get: |e| a(e)?.p95_ms,
            },
        ],
        area_under_first: false,
        skip_if_all_zero: false,
    },
    PanelDef {
        slug: "finality-percentiles",
        y_title: "ms",
        series: &[
            SeriesDef {
                name: "p50",
                color: 0,
                get: |e| p(e)?.finalization_p50_ms,
            },
            SeriesDef {
                name: "p95",
                color: 1,
                get: |e| p(e)?.finalization_p95_ms,
            },
            SeriesDef {
                name: "p99",
                color: 2,
                get: |e| p(e)?.finalization_p99_ms,
            },
        ],
        area_under_first: false,
        skip_if_all_zero: false,
    },
    PanelDef {
        slug: "too-far-ahead",
        y_title: "rejections / run",
        series: &[SeriesDef {
            name: "total",
            color: 0,
            get: |e| p(e)?.too_far_ahead_errors,
        }],
        area_under_first: false,
        skip_if_all_zero: true,
    },
    PanelDef {
        slug: "lfb-spread",
        y_title: "blocks",
        series: &[
            SeriesDef {
                name: "p95",
                color: 1,
                get: |e| lfb(e)?.p95,
            },
            SeriesDef {
                name: "max",
                color: 2,
                get: |e| lfb(e)?.max,
            },
        ],
        area_under_first: false,
        skip_if_all_zero: false,
    },
];

/// One observation in long form: the run's date (temporal axis + short label
/// for sparse categorical axes) with a series' value on that date.
struct Obs {
    when: ctime::OffsetDateTime,
    label: String,
    series: &'static str,
    value: f64,
}

fn parse_date(raw: &str) -> Option<(ctime::OffsetDateTime, String)> {
    let b = raw.as_bytes();
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let year: i32 = raw.get(0..4)?.parse().ok()?;
    let month: u8 = raw.get(5..7)?.parse().ok()?;
    let day: u8 = raw.get(8..10)?.parse().ok()?;
    let date =
        ctime::Date::from_calendar_date(year, ctime::Month::try_from(month).ok()?, day).ok()?;
    Some((date.midnight().assume_utc(), raw[5..10].to_string()))
}

fn collect_panel(history: &[Entry], panel: &PanelDef) -> Vec<Obs> {
    let mut out = Vec::new();
    for e in history {
        let Some((when, label)) = e
            .run
            .as_ref()
            .and_then(|r| r.date.as_deref())
            .and_then(parse_date)
        else {
            continue;
        };
        for s in panel.series {
            if let Some(v) = (s.get)(e) {
                if v.is_finite() {
                    out.push(Obs {
                        when,
                        label: label.clone(),
                        series: s.name,
                        value: v,
                    });
                }
            }
        }
    }
    out
}

/// Value labels for sparse marks: exact numbers are the point of an annotated
/// bar, so keep integers integral and give small values two decimals.
fn fmt_value(v: f64) -> String {
    if v >= 100.0 || (v.fract() == 0.0 && v.abs() >= 1.0) {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
    }
}

fn hex_rgba(hex: &str, alpha: f64) -> String {
    let h = hex.trim_start_matches('#');
    let n = u32::from_str_radix(h, 16).unwrap_or(0);
    format!(
        "rgba({},{},{},{alpha:.2})",
        (n >> 16) & 0xff,
        (n >> 8) & 0xff,
        n & 0xff
    )
}

fn base_theme(t: charton::theme::Theme, theme: &Theme) -> charton::theme::Theme {
    t.with_show_legend(false)
        .with_x_tick_label_angle(45.0)
        .with_background_color(theme.surface)
        .with_label_color(theme.axis_ink)
        .with_tick_label_color(theme.axis_ink)
}

/// Layered line + point per series on a temporal axis. Layer order puts every
/// line under every point; mark-level colors keep each series on its fixed
/// palette slot without a color channel, so layer scale unification never has
/// competing color scales to fight over (the same trick the heatmap's text
/// layers use).
fn render_dense(
    panel: &PanelDef,
    obs: &[Obs],
    theme: &Theme,
    out: &str,
) -> Result<(), Box<dyn Error>> {
    let mut layers: Vec<LayeredChart> = Vec::new();
    for s in panel.series {
        let pts: Vec<&Obs> = obs.iter().filter(|o| o.series == s.name).collect();
        if pts.is_empty() {
            continue;
        }
        let dates: Vec<ctime::OffsetDateTime> = pts.iter().map(|o| o.when).collect();
        let values: Vec<f64> = pts.iter().map(|o| o.value).collect();
        let color = theme.series[s.color];
        let ds = || -> Result<Dataset, Box<dyn Error>> {
            Ok(Dataset::new()
                .with_column("date", dates.clone())?
                .with_column(panel.y_title, values.clone())?)
        };
        let encode = |c| -> (_, _) {
            (
                alt::x("date").with_scale(Scale::Temporal),
                alt::y(c).with_zero(true),
            )
        };
        if panel.area_under_first && s.name == panel.series[0].name && pts.len() > 1 {
            let fill = hex_rgba(color, if theme.zero_is_dark { 0.20 } else { 0.14 });
            layers.push(
                Chart::build(ds()?)?
                    .mark_area()?
                    .configure_area(|ar| ar.with_color(fill.as_str()).with_stroke("none"))
                    .encode(encode(panel.y_title))?
                    .configure_theme(|t| base_theme(t, theme)),
            );
        }
        if pts.len() > 1 {
            layers.push(
                Chart::build(ds()?)?
                    .mark_line()?
                    .configure_line(|l| l.with_color(color))
                    .encode(encode(panel.y_title))?
                    .configure_theme(|t| base_theme(t, theme)),
            );
        }
        layers.push(
            Chart::build(ds()?)?
                .mark_point()?
                .configure_point(|pt| {
                    pt.with_color(color)
                        .with_size(4.0)
                        .with_stroke(theme.surface)
                        .with_stroke_width(1.5)
                })
                .encode(encode(panel.y_title))?
                .configure_theme(|t| base_theme(t, theme)),
        );
    }
    save_layers(layers, theme, out, false)
}

/// Sparse rendering: value-labelled marks instead of a mostly-empty line.
/// One layer per series with mark-level color; the x axis is categorical —
/// dates when a single series (or several series across several dates, where
/// points replace bars so nothing overlaps), series names when several series
/// share the one recorded date.
fn render_sparse(
    panel: &PanelDef,
    obs: &[Obs],
    n_dates: usize,
    theme: &Theme,
    out: &str,
) -> Result<(), Box<dyn Error>> {
    let multi = panel.series.len() > 1;
    let by_series_x = multi && n_dates == 1;
    let bars = !multi || by_series_x;
    // Column names become axis titles: when the one recorded date puts series
    // names on the x axis, calling that axis "date" would be a small lie.
    let x_title = if by_series_x { "latest run" } else { "date" };
    // Sparse axes hold at most a handful of short categories, so tick labels
    // stay horizontal — the 45° angle the dense/heatmap charts need would run
    // rotated labels through the bars here.
    let sparse_theme =
        |t: charton::theme::Theme, th: &Theme| base_theme(t, th).with_x_tick_label_angle(0.0);
    let mut layers: Vec<LayeredChart> = Vec::new();
    for s in panel.series {
        let pts: Vec<&Obs> = obs.iter().filter(|o| o.series == s.name).collect();
        if pts.is_empty() {
            continue;
        }
        let x_vals: Vec<String> = pts
            .iter()
            .map(|o| {
                if by_series_x {
                    o.series.to_string()
                } else {
                    o.label.clone()
                }
            })
            .collect();
        let values: Vec<f64> = pts.iter().map(|o| o.value).collect();
        let labels: Vec<String> = pts.iter().map(|o| fmt_value(o.value)).collect();
        let color = theme.series[s.color];
        let ds = Dataset::new()
            .with_column(x_title, x_vals.clone())?
            .with_column(panel.y_title, values.clone())?;
        layers.push(if bars {
            Chart::build(ds)?
                .mark_bar()?
                .configure_bar(|b| b.with_color(color))
                .encode((alt::x(x_title), alt::y(panel.y_title).with_zero(true)))?
                .configure_theme(|t| sparse_theme(t, theme))
        } else {
            Chart::build(ds)?
                .mark_point()?
                .configure_point(|pt| {
                    pt.with_color(color)
                        .with_size(6.5)
                        .with_stroke(theme.surface)
                        .with_stroke_width(1.5)
                })
                .encode((alt::x(x_title), alt::y(panel.y_title).with_zero(true)))?
                .configure_theme(|t| sparse_theme(t, theme))
        });
        // The exact number is the annotation's job — a sparse chart that makes
        // the reader guess a bar height has no advantage over the table. The
        // label rides 6% above its mark: centered on the value it would
        // straddle the bar top and, at the series maximum, clip against the
        // plot edge — the offset both clears the mark and stretches the
        // unified y domain into headroom for the topmost label.
        let ink = if theme.zero_is_dark {
            theme.ink_on_dark
        } else {
            theme.ink_on_light
        };
        let raised: Vec<f64> = values.iter().map(|v| v * 1.06).collect();
        let ds = Dataset::new()
            .with_column(x_title, x_vals)?
            .with_column(panel.y_title, raised)?
            .with_column("label", labels)?;
        layers.push(
            Chart::build(ds)?
                .mark_text()?
                .configure_text(|t| t.with_size(11.0).with_color(ink))
                .encode((
                    alt::x(x_title),
                    alt::y(panel.y_title).with_zero(true),
                    alt::text("label"),
                ))?
                .configure_theme(|t| sparse_theme(t, theme)),
        );
    }
    save_layers(layers, theme, out, false)
}

fn render_panel(
    panel: &PanelDef,
    history: &[Entry],
    theme: &Theme,
    out: &str,
) -> Result<bool, Box<dyn Error>> {
    // The CPU panel is a core × time matrix, not a fixed series list — its
    // facets come from the data, so it has its own renderer.
    if panel.slug == "peak-cpu" {
        return render_cpu_panel(history, theme, out);
    }
    let obs = collect_panel(history, panel);
    if obs.is_empty() {
        return Ok(false);
    }
    if panel.skip_if_all_zero && obs.iter().all(|o| o.value == 0.0) {
        return Ok(false);
    }
    let mut dates: Vec<ctime::OffsetDateTime> = obs.iter().map(|o| o.when).collect();
    dates.sort();
    dates.dedup();
    if dates.len() > SPARSE_MAX_DATES {
        render_dense(panel, &obs, theme, out)?;
    } else {
        render_sparse(panel, &obs, dates.len(), theme, out)?;
    }
    Ok(true)
}

/// One CPU reading: a core's peak utilization on a run date.
struct CpuObs {
    when: ctime::OffsetDateTime,
    label: String,
    core: String,
    pct: f64,
}

/// Long-form core × time extraction. Per-core readings win when present;
/// otherwise the run contributes a single synthesized "aggregate" row, so the
/// panel stays useful today and grows facets the moment per-core data lands —
/// with no change to the visual specification.
fn collect_cpu(history: &[Entry]) -> Vec<CpuObs> {
    let mut out = Vec::new();
    for e in history {
        let Some((when, label)) = e
            .run
            .as_ref()
            .and_then(|r| r.date.as_deref())
            .and_then(parse_date)
        else {
            continue;
        };
        let Some(passive) = p(e) else { continue };
        let per_core = passive
            .cpu_peak_per_core_pct
            .as_ref()
            .filter(|m| !m.is_empty());
        if let Some(m) = per_core {
            for (core, pct) in m {
                if pct.is_finite() {
                    out.push(CpuObs {
                        when,
                        label: label.clone(),
                        core: core.clone(),
                        pct: *pct,
                    });
                }
            }
        } else if let Some(v) = passive.cpu_peak_pct {
            if v.is_finite() {
                out.push(CpuObs {
                    when,
                    label: label.clone(),
                    core: "aggregate".to_string(),
                    pct: v,
                });
            }
        }
    }
    out
}

const CPU_Y_TITLE: &str = "% of one core";

/// Small-multiples CPU panel: one sub-chart per core, stacked vertically into
/// a single SVG (charton 0.5's declarative facet entry point is not public,
/// so this is the spec's sanctioned fallback — repeated charts on a common
/// scale). All facets share the y domain, pinned to reach past 100% so the
/// dashed saturation line and its headroom are always visible, and the
/// threshold line's endpoints span the global date range, which also pins
/// every facet's x domain to the same span. With only today's aggregate data
/// this degenerates to exactly one facet.
fn render_cpu_panel(history: &[Entry], theme: &Theme, out: &str) -> Result<bool, Box<dyn Error>> {
    let obs = collect_cpu(history);
    if obs.is_empty() {
        return Ok(false);
    }
    let mut cores: Vec<String> = obs.iter().map(|o| o.core.clone()).collect();
    cores.sort();
    cores.dedup();
    let mut dates: Vec<(ctime::OffsetDateTime, String)> =
        obs.iter().map(|o| (o.when, o.label.clone())).collect();
    dates.sort();
    dates.dedup();
    let n_dates = dates.len();
    let data_max = obs.iter().map(|o| o.pct).fold(0.0_f64, f64::max);
    // Past the saturation line, plus headroom for sparse value labels.
    let y_top = data_max.max(100.0) * 1.12;

    let mut docs: Vec<String> = Vec::new();
    for core in &cores {
        let pts: Vec<&CpuObs> = obs.iter().filter(|o| &o.core == core).collect();
        let layered = build_cpu_facet(&pts, &dates, n_dates, y_top, theme)?;
        docs.push(retheme_str(layered.to_svg()?, theme, false));
    }
    if docs.len() == 1 {
        fs::write(out, &docs[0])?;
    } else {
        fs::write(out, stack_facets(&docs, &cores, theme))?;
    }
    Ok(true)
}

fn build_cpu_facet(
    pts: &[&CpuObs],
    all_dates: &[(ctime::OffsetDateTime, String)],
    n_dates: usize,
    y_top: f64,
    theme: &Theme,
) -> Result<LayeredChart, Box<dyn Error>> {
    let dense = n_dates > SPARSE_MAX_DATES;
    let color = theme.series[0];
    let y = || {
        alt::y(CPU_Y_TITLE)
            .with_zero(true)
            .with_domain(ScaleDomain::Continuous(0.0, y_top))
    };
    let mut layers: Vec<LayeredChart> = Vec::new();

    // Saturation line first (under the data): a dashed status-serious line at
    // 100% — one full core — so multi-core load is legible at a glance.
    // mark_rule only draws vertical rules, hence a two-point line. Its
    // endpoints cover the GLOBAL date span, which pins each facet's x domain
    // to the same range — the shared-x guarantee for the stack. Needs two
    // distinct x positions, so the single-date case leaves the threshold to
    // the pinned y domain alone.
    if n_dates >= 2 {
        let rule_theme = |t: charton::theme::Theme, th: &Theme| {
            base_theme(t, th).with_x_tick_label_angle(if dense { 45.0 } else { 0.0 })
        };
        if dense {
            let ds = Dataset::new()
                .with_column("date", vec![all_dates[0].0, all_dates[n_dates - 1].0])?
                .with_column(CPU_Y_TITLE, vec![100.0, 100.0])?;
            layers.push(
                Chart::build(ds)?
                    .mark_line()?
                    .configure_line(|l| l.with_color(theme.serious).with_dash(vec![5.0, 5.0]))
                    .encode((alt::x("date").with_scale(Scale::Temporal), y()))?
                    .configure_theme(|t| rule_theme(t, theme)),
            );
        } else {
            // Categorical x: a y=100 point on every date label keeps the line
            // horizontal across the whole band.
            let ds = Dataset::new()
                .with_column(
                    "date",
                    all_dates.iter().map(|d| d.1.clone()).collect::<Vec<_>>(),
                )?
                .with_column(CPU_Y_TITLE, vec![100.0; n_dates])?;
            layers.push(
                Chart::build(ds)?
                    .mark_line()?
                    .configure_line(|l| l.with_color(theme.serious).with_dash(vec![5.0, 5.0]))
                    .encode((alt::x("date"), y()))?
                    .configure_theme(|t| rule_theme(t, theme)),
            );
        }
    }

    if dense {
        let ds = || -> Result<Dataset, Box<dyn Error>> {
            Ok(Dataset::new()
                .with_column("date", pts.iter().map(|o| o.when).collect::<Vec<_>>())?
                .with_column(CPU_Y_TITLE, pts.iter().map(|o| o.pct).collect::<Vec<_>>())?)
        };
        if pts.len() > 1 {
            layers.push(
                Chart::build(ds()?)?
                    .mark_line()?
                    .configure_line(|l| l.with_color(color))
                    .encode((alt::x("date").with_scale(Scale::Temporal), y()))?
                    .configure_theme(|t| base_theme(t, theme)),
            );
        }
        layers.push(
            Chart::build(ds()?)?
                .mark_point()?
                .configure_point(|pt| {
                    pt.with_color(color)
                        .with_size(4.0)
                        .with_stroke(theme.surface)
                        .with_stroke_width(1.5)
                })
                .encode((alt::x("date").with_scale(Scale::Temporal), y()))?
                .configure_theme(|t| base_theme(t, theme)),
        );
    } else {
        let sparse_theme =
            |t: charton::theme::Theme, th: &Theme| base_theme(t, th).with_x_tick_label_angle(0.0);
        let x_vals: Vec<String> = pts.iter().map(|o| o.label.clone()).collect();
        let values: Vec<f64> = pts.iter().map(|o| o.pct).collect();
        let ds = Dataset::new()
            .with_column("date", x_vals.clone())?
            .with_column(CPU_Y_TITLE, values.clone())?;
        layers.push(
            Chart::build(ds)?
                .mark_point()?
                .configure_point(|pt| {
                    pt.with_color(color)
                        .with_size(6.5)
                        .with_stroke(theme.surface)
                        .with_stroke_width(1.5)
                })
                .encode((alt::x("date"), y()))?
                .configure_theme(|t| sparse_theme(t, theme)),
        );
        let ink = if theme.zero_is_dark {
            theme.ink_on_dark
        } else {
            theme.ink_on_light
        };
        let raised: Vec<f64> = values.iter().map(|v| v * 1.06).collect();
        let labels: Vec<String> = values.iter().map(|v| fmt_value(*v)).collect();
        let ds = Dataset::new()
            .with_column("date", x_vals)?
            .with_column(CPU_Y_TITLE, raised)?
            .with_column("label", labels)?;
        layers.push(
            Chart::build(ds)?
                .mark_text()?
                .configure_text(|t| t.with_size(11.0).with_color(ink))
                .encode((alt::x("date"), y(), alt::text("label")))?
                .configure_theme(|t| sparse_theme(t, theme)),
        );
    }

    let mut iter = layers.into_iter();
    let first = iter.next().ok_or("no layers to render")?;
    Ok(iter.fold(first, |acc, layer| acc.and(layer)))
}

/// Stack per-core facet SVGs vertically into one document. Each facet's clip
/// path id is uniquified (charton always emits `plot-clip-area`, which would
/// collide across facets and break clipping) and gains a core label.
fn stack_facets(docs: &[String], cores: &[String], theme: &Theme) -> String {
    let (w, h) = (500, 400);
    let mut inner = String::new();
    for (i, (doc, core)) in docs.iter().zip(cores).enumerate() {
        let body = doc
            .find('>')
            .map(|start| &doc[start + 1..])
            .unwrap_or(doc)
            .trim_end()
            .trim_end_matches("</svg>");
        let body = body.replace("plot-clip-area", &format!("plot-clip-{i}"));
        inner.push_str(&format!(
            "<g transform=\"translate(0,{})\">{}<text x=\"{}\" y=\"16\" \
             text-anchor=\"middle\" font-size=\"12\" font-weight=\"600\" \
             font-family=\"sans-serif\" fill=\"{}\">{}</text></g>",
            i * h,
            body,
            w / 2,
            theme.axis_ink,
            esc_xml(core),
        ));
    }
    format!(
        "<svg width=\"{w}\" height=\"{}\" viewBox=\"0 0 {w} {}\" \
         xmlns=\"http://www.w3.org/2000/svg\">{inner}</svg>",
        docs.len() * h,
        docs.len() * h,
    )
}

/// Core ids come from published JSON — escape them before they become SVG
/// text, same stance as the dashboard page's esc().
fn esc_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

struct HeatCell {
    date: String,
    category: &'static str,
    rate: f64,
    failures: f64,
    iterations: f64,
}

fn collect_heat_cells(history: &[Entry]) -> Vec<HeatCell> {
    let mut cells = Vec::new();
    for e in history {
        let date = match e.run.as_ref().and_then(|r| r.date.as_ref()) {
            Some(d) if d.len() >= 10 => d[5..10].to_string(),
            _ => continue,
        };
        let Some(p) = e.passive.as_ref() else {
            continue;
        };
        let mut push = |category: &'static str, iters: Option<f64>, fails: Option<f64>| {
            let (Some(i), Some(f)) = (iters, fails) else {
                return;
            };
            if i <= 0.0 {
                return;
            }
            cells.push(HeatCell {
                date: date.clone(),
                category,
                rate: f / i,
                failures: f,
                iterations: i,
            });
        };
        push("total", p.iterations, p.failures);
        if let Some(pr) = p.providers.as_ref() {
            if let Some(d) = pr.docker.as_ref() {
                push("docker", d.iterations, d.failures);
            }
            if let Some(s) = pr.subprocess.as_ref() {
                push("subprocess", s.iterations, s.failures);
            }
        }
    }
    cells
}

fn render_heatmap(cells: &[HeatCell], theme: &Theme, out: &str) -> Result<(), Box<dyn Error>> {
    // mark_rect demands a color channel and refuses Discrete scales, so all
    // cells (zeros included) ride one continuous Reds layer: with the domain
    // spanning [0, max], a zero-rate cell normalizes to exactly 0.0 and
    // renders exactly the ramp's start color. save() then rewrites that one
    // rect fill to the theme's neutral (see retheme_svg) — the declarative
    // API has no per-cell override, and a separate constant-value layer
    // degenerates in normalization (renders the mapper's black fallback).
    // Legend stays suppressed: the dashboard renders its own, labelled with
    // the real min/max rates it computes from history.json.
    let max_rate = cells.iter().map(|c| c.rate).fold(0.0_f64, f64::max);

    let col = |sel: &[&HeatCell], f: &dyn Fn(&HeatCell) -> String| -> Vec<String> {
        sel.iter().map(|c| f(c)).collect()
    };
    let label = |c: &HeatCell| format!("{}/{}", c.failures as i64, c.iterations as i64);

    let all: Vec<&HeatCell> = cells.iter().collect();
    let ds = Dataset::new()
        .with_column("date", col(&all, &|c| c.date.clone()))?
        .with_column("category", col(&all, &|c| c.category.to_string()))?
        .with_column("rate", cells.iter().map(|c| c.rate).collect::<Vec<f64>>())?;
    let mut layers: Vec<LayeredChart> = vec![Chart::build(ds)?
        .mark_rect()?
        .encode((
            alt::x("date"),
            alt::y("category"),
            alt::color("rate")
                .with_domain(ScaleDomain::Continuous(
                    0.0,
                    if max_rate > 0.0 { max_rate } else { 1.0 },
                ))
                .with_expandsion(Expansion {
                    mult: (0.0, 0.0),
                    add: (0.0, 0.0),
                }),
        ))?
        .configure_theme(|t| base_theme(t, theme).with_color_map(theme.ramp))];

    // "f/n" volume on every cell, split into two ink layers by the darkness
    // of the fill under the text — the ramp's upper half is dark in both
    // themes, and the zero-neutral is dark only on the dark theme. The ink is
    // set at the mark level (configure_text with_color) rather than through a
    // color channel: layered charts unify non-positional scales, so a
    // Discrete text-color scale would conflict with the rects' Linear ramp.
    let dark_fill = |c: &HeatCell| {
        (c.rate <= 0.0 && theme.zero_is_dark)
            || (c.rate > 0.0 && max_rate > 0.0 && c.rate / max_rate > 0.55)
    };
    for (on_dark, ink) in [(false, theme.ink_on_light), (true, theme.ink_on_dark)] {
        let sel: Vec<&HeatCell> = cells.iter().filter(|c| dark_fill(c) == on_dark).collect();
        if sel.is_empty() {
            continue;
        }
        let ds = Dataset::new()
            .with_column("date", col(&sel, &|c| c.date.clone()))?
            .with_column("category", col(&sel, &|c| c.category.to_string()))?
            .with_column("label", col(&sel, &|c| label(c)))?;
        layers.push(
            Chart::build(ds)?
                .mark_text()?
                .configure_text(|t| t.with_size(10.0).with_color(ink))
                .encode((alt::x("date"), alt::y("category"), alt::text("label")))?
                .configure_theme(|t| base_theme(t, theme)),
        );
    }

    save_layers(layers, theme, out, true)?;
    if max_rate == 0.0 {
        let svg = fs::read_to_string(out)?;
        fs::write(
            out,
            svg.replace(
                "fill=\"rgba(251,106,74,1.000)\"",
                &format!("fill=\"{}\"", theme.zero_neutral),
            ),
        )?;
    }
    Ok(())
}

fn save_layers(
    layers: Vec<LayeredChart>,
    theme: &Theme,
    out: &str,
    neutralize_zero: bool,
) -> Result<(), Box<dyn Error>> {
    let mut iter = layers.into_iter();
    let first = iter.next().ok_or("no layers to render")?;
    let layered = iter.fold(first, |acc, layer| acc.and(layer));
    layered.save(out)?;
    retheme_svg(out, theme, neutralize_zero)?;
    Ok(())
}

/// Exact-color rewrites the declarative API has no hook for.
///
/// Zero cells (heatmap only): a zero-rate cell normalizes to 0.0 and renders
/// precisely the ramp's start color; substituting that one fill realizes "0%
/// is a neutral non-red" without a per-cell API. A tiny nonzero rate that
/// ROUNDS to the same 8-bit color would be caught too — visually
/// indistinguishable either way, and the cell's f/n text disambiguates.
///
/// Axis lines (every chart): charton's theme colors axis TEXT but not the
/// axis lines and tick marks, which keep its hardcoded rgba(51,51,51)
/// stroke — near-invisible on the dark surface. That exact grey appears
/// nowhere in the Reds ramp or either theme's palette, so a stroke-scoped
/// rewrite is unambiguous.
fn retheme_svg(path: &str, theme: &Theme, neutralize_zero: bool) -> Result<(), Box<dyn Error>> {
    let svg = fs::read_to_string(path)?;
    fs::write(path, retheme_str(svg, theme, neutralize_zero))?;
    Ok(())
}

fn retheme_str(svg: String, theme: &Theme, neutralize_zero: bool) -> String {
    let replaced = svg.replace(
        "stroke=\"rgba(51,51,51,1.000)\"",
        &format!("stroke=\"{}\"", theme.axis_ink),
    );
    if neutralize_zero {
        replaced.replace(
            &format!("fill=\"{}\"", theme.ramp_start),
            &format!("fill=\"{}\"", theme.zero_neutral),
        )
    } else {
        replaced
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let mut history_path = None;
    let mut out_dir = None;
    let mut series = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--history" => history_path = args.next(),
            "--out-dir" => out_dir = args.next(),
            "--series" => series = args.next(),
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }
    let history_path = history_path.ok_or("--history is required")?;
    let out_dir = out_dir.ok_or("--out-dir is required")?;
    let series = series.ok_or("--series is required")?;
    if series != "weekend" && series != "daily" {
        return Err(format!("--series must be weekend or daily, got {series}").into());
    }

    let raw = fs::read_to_string(&history_path)?;
    let history: Vec<Entry> = serde_json::from_str(&raw)?;
    fs::create_dir_all(&out_dir)?;

    let mut written: Vec<String> = Vec::new();

    let cells = collect_heat_cells(&history);
    if cells.is_empty() {
        eprintln!(
            "no renderable heatmap cells in {history_path}; skipping failure-heatmap-{series}"
        );
    } else {
        for theme in &THEMES {
            let name = format!("failure-heatmap-{series}-{}.svg", theme.name);
            render_heatmap(&cells, theme, &format!("{out_dir}/{name}"))?;
            println!("wrote {out_dir}/{name}");
            written.push(name);
        }
    }

    for panel in &PANELS {
        for theme in &THEMES {
            let name = format!("chart-{}-{series}-{}.svg", panel.slug, theme.name);
            let path = format!("{out_dir}/{name}");
            if render_panel(panel, &history, theme, &path)? {
                println!("wrote {path}");
                written.push(name);
            } else if theme.name == THEMES[0].name {
                eprintln!(
                    "no data for {} in {history_path}; skipping chart-{}-{series}",
                    panel.slug, panel.slug
                );
            }
        }
    }

    // The manifest is the publishers' carry-forward list: it names exactly the
    // SVGs this render produced, so a chart added or suppressed here changes
    // what publishers preserve without any workflow edit. Written even when
    // empty — an empty array is the honest bootstrap state.
    let manifest = format!("{out_dir}/charts-manifest-{series}.json");
    fs::write(&manifest, serde_json::to_string_pretty(&written)?)?;
    println!("wrote {manifest}");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("soak-charts: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
