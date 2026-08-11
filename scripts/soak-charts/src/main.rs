//! Publish-time chart renderer for the soak dashboard.
//!
//! Reads a soak history series (history.json / history-daily.json) and emits
//! static SVG charts into the Pages site tree. Run by the publishers whose
//! output can change history — the final soak publish and dashboard edits;
//! checkpoint publishes carry the previous SVGs forward unchanged, because a
//! checkpoint never appends to history.
//!
//! First chart: the failure heatmap. Rows are failure categories (total plus
//! per provider), columns are run dates, cell color is the failure rate.
//! Rates are binned into discrete classes rather than mapped to a continuous
//! ramp: the classes give a legible legend, exact palette control, and — the
//! part a continuous scale gets wrong — a 0% class in a neutral non-red so
//! "ran clean" can never be misread as "barely failed". Dates a category
//! simply did not run leave no cell at all, so absence and zero stay visually
//! distinct. Cell text carries volume ("3/176") since a rate alone hides how
//! much evidence is behind it.
//!
//! Colors are baked into the SVG at render time, so each chart is emitted in
//! a light and a dark variant and the page swaps them with the same theme
//! logic as the rest of the dashboard. Both red ramps are sequential with
//! monotonic lightness on their surface (validated against the dataviz
//! palette checks).

use std::env;
use std::error::Error;
use std::fs;
use std::process::ExitCode;

use charton::prelude::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Entry {
    run: Option<Run>,
    passive: Option<Passive>,
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

struct Cell {
    date: String,
    category: &'static str,
    rate: f64,
    failures: f64,
    iterations: f64,
}

struct Theme {
    name: &'static str,
    /// Sequential ramp for nonzero failure rates (light -> dark red).
    ramp: ColorMap,
    /// The exact fill charton emits for a rate normalizing to 0.0 on `ramp` —
    /// the substitution target of neutralize_zero_fill. ColorBrewer Reds
    /// starts at rgb(255,245,240).
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
    },
    Theme {
        name: "dark",
        ramp: ColorMap::Reds,
        ramp_start: "rgba(255,245,240,1.000)",
        zero_neutral: "#33322f",
        zero_is_dark: true,
        ink_on_light: "#1a1a19",
        ink_on_dark: "#e8e7df",
    },
];

fn collect_cells(history: &[Entry]) -> Vec<Cell> {
    let mut cells = Vec::new();
    for e in history {
        let date = match e.run.as_ref().and_then(|r| r.date.as_ref()) {
            Some(d) if d.len() >= 10 => d[5..10].to_string(),
            _ => continue,
        };
        let Some(p) = e.passive.as_ref() else { continue };
        let mut push = |category: &'static str, iters: Option<f64>, fails: Option<f64>| {
            let (Some(i), Some(f)) = (iters, fails) else { return };
            if i <= 0.0 {
                return;
            }
            cells.push(Cell {
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

fn render(cells: &[Cell], theme: &Theme, out: &str) -> Result<(), Box<dyn Error>> {
    // mark_rect demands a color channel and refuses Discrete scales, so all
    // cells (zeros included) ride one continuous Reds layer: with the domain
    // spanning [0, max], a zero-rate cell normalizes to exactly 0.0 and
    // renders exactly the ramp's start color. save() then rewrites that one
    // rect fill to the theme's neutral (see neutralize_zero_fill) — the
    // declarative API has no per-cell override, and a separate constant-value
    // layer degenerates in normalization (renders the mapper's black
    // fallback). Legend stays suppressed: the dashboard renders its own,
    // labelled with the real min/max rates it computes from history.json.
    let max_rate = cells.iter().map(|c| c.rate).fold(0.0_f64, f64::max);

    let col = |sel: &[&Cell], f: &dyn Fn(&Cell) -> String| -> Vec<String> {
        sel.iter().map(|c| f(c)).collect()
    };
    let label = |c: &Cell| format!("{}/{}", c.failures as i64, c.iterations as i64);

    let all: Vec<&Cell> = cells.iter().collect();
    let ds = Dataset::new()
        .with_column("date", col(&all, &|c| c.date.clone()))?
        .with_column("category", col(&all, &|c| c.category.to_string()))?
        .with_column("rate", cells.iter().map(|c| c.rate).collect::<Vec<f64>>())?;
    let mut layers: Vec<LayeredChart> = vec![Chart::build(ds)?
        .mark_rect()?
        .encode((alt::x("date"), alt::y("category"), alt::color("rate")))?
        .configure_theme(|t| {
            t.with_color_map(theme.ramp)
                .with_show_legend(false)
                .with_x_tick_label_angle(45.0)
        })];

    // "f/n" volume on every cell, split into two ink layers by the darkness
    // of the fill under the text — the ramp's upper half is dark in both
    // themes, and the zero-neutral is dark only on the dark theme. The ink is
    // set at the mark level (configure_text with_color) rather than through a
    // color channel: layered charts unify non-positional scales, so a
    // Discrete text-color scale would conflict with the rects' Linear ramp.
    let dark_fill = |c: &Cell| {
        (c.rate <= 0.0 && theme.zero_is_dark)
            || (c.rate > 0.0 && max_rate > 0.0 && c.rate / max_rate > 0.55)
    };
    for (on_dark, ink) in [(false, theme.ink_on_light), (true, theme.ink_on_dark)] {
        let sel: Vec<&Cell> = cells
            .iter()
            .filter(|c| dark_fill(c) == on_dark)
            .collect();
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
                .configure_theme(|t| {
                    t.with_show_legend(false).with_x_tick_label_angle(45.0)
                }),
        );
    }

    let mut iter = layers.into_iter();
    let first = iter.next().ok_or("no layers to render")?;
    let layered = iter.fold(first, |acc, layer| acc.and(layer));
    layered.save(out)?;
    neutralize_zero_fill(out, theme)?;
    Ok(())
}

/// Rewrite the ramp's exact start color to the theme's neutral, on rect fills
/// only. A zero-rate cell normalizes to 0.0 and therefore renders precisely
/// the ramp start; substituting that one fill realizes "0% is a neutral
/// non-red" without a per-cell API. A tiny nonzero rate that ROUNDS to the
/// same 8-bit color would be caught too — visually indistinguishable either
/// way, and the cell's f/n text disambiguates.
fn neutralize_zero_fill(path: &str, theme: &Theme) -> Result<(), Box<dyn Error>> {
    let svg = fs::read_to_string(path)?;
    let replaced = svg.replace(
        &format!("fill=\"{}\"", theme.ramp_start),
        &format!("fill=\"{}\"", theme.zero_neutral),
    );
    fs::write(path, replaced)?;
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let mut history_path = None;
    let mut out_dir = None;
    let mut basename = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--history" => history_path = args.next(),
            "--out-dir" => out_dir = args.next(),
            "--basename" => basename = args.next(),
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }
    let history_path = history_path.ok_or("--history is required")?;
    let out_dir = out_dir.ok_or("--out-dir is required")?;
    let basename = basename.ok_or("--basename is required")?;

    let raw = fs::read_to_string(&history_path)?;
    let history: Vec<Entry> = serde_json::from_str(&raw)?;
    let cells = collect_cells(&history);
    if cells.is_empty() {
        // An empty series is the bootstrap state, not an error: emit nothing
        // and let the dashboard's empty-state text stand.
        eprintln!("no renderable cells in {history_path}; skipping {basename}");
        return Ok(());
    }

    fs::create_dir_all(&out_dir)?;
    for theme in &THEMES {
        let out = format!("{out_dir}/{basename}-{}.svg", theme.name);
        render(&cells, theme, &out)?;
        println!("wrote {out}");
    }
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
