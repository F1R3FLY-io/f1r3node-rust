//! Local preview server and fixture generator for the soak dashboard.
//!
//! Built with plain `rustc` against std only — no crates, no Cargo.toml, and
//! nothing added to the workspace. The pinned nightly in rust-toolchain.toml is
//! the one toolchain every developer here is guaranteed to have, so the preview
//! must not depend on anything else being installed.
//!
//! Usage (driven by scripts/preview-soak-dashboard.sh):
//!   preview --dir <site-dir> --port <n> [--seed sample|empty|none]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

// ---------------------------------------------------------------- utilities

/// Deterministic LCG. Fixtures must be byte-identical run to run, otherwise a
/// visual diff of a dashboard change is indistinguishable from data noise.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    fn unif(&mut self, lo: f64, hi: f64) -> f64 {
        let x = (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        lo + x * (hi - lo)
    }
    fn sha(&mut self) -> String {
        (0..40)
            .map(|_| char::from_digit((self.next_u64() >> 20) as u32 % 16, 16).unwrap())
            .collect()
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn iso_date(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn r(v: f64, places: i32) -> f64 {
    let f = 10f64.powi(places);
    (v * f).round() / f
}

// ---------------------------------------------------------------- fixtures

struct Run {
    date: String,
    run_id: u64,
    sha: String,
    kind: &'static str,
    target_ref: &'static str,
    duration: u64,
    iterations: u64,
    failure_rate: f64,
    iters_per_hour: f64,
    rss_mb: f64,
    final_p95: f64,
    docker: (u64, u64),
    subprocess: (u64, u64),
    active: bool,
}

impl Run {
    fn to_json(&self) -> String {
        let active = if self.active {
            format!(
                r#"{{"p95_ms": {}, "p50_ms": {}, "rss_peak_mb": {}, "throughput": {}, "segments": 14, "segment_failures": 0}}"#,
                r(self.final_p95 * 1.14, 1),
                r(self.final_p95 * 0.42, 1),
                r(self.rss_mb * 1.06, 1),
                r(self.iters_per_hour * 4.2, 2)
            )
        } else {
            "null".to_string()
        };
        format!(
            concat!(
                r#"{{"run": {{"date": "{}T07:30:00Z", "run_id": "{}", "target_sha": "{}", "#,
                r#""target_ref": "{}", "kind": "{}", "duration_seconds": {}}}, "#,
                r#""passive": {{"iterations": {}, "failures": {}, "failure_rate": {}, "#,
                r#""iterations_per_hour": {}, "rss_peak_mb": {}, "finalization_p95_ms": {}, "#,
                r#""providers": {{"docker": {{"iterations": {}, "failures": {}}}, "#,
                r#""subprocess": {{"iterations": {}, "failures": {}}}}}}}, "active": {}}}"#
            ),
            self.date,
            self.run_id,
            self.sha,
            self.target_ref,
            self.kind,
            self.duration,
            self.iterations,
            (self.iterations as f64 * self.failure_rate) as u64,
            r(self.failure_rate, 4),
            r(self.iters_per_hour, 3),
            r(self.rss_mb, 1),
            r(self.final_p95, 1),
            self.docker.0,
            self.docker.1,
            self.subprocess.0,
            self.subprocess.1,
            active
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn series(
    rng: &mut Rng,
    start: i64,
    count: usize,
    step_days: i64,
    kind: &'static str,
    target_ref: &'static str,
    duration: u64,
    hours: f64,
    mut rss: f64,
    mut p95: f64,
    mut iph: f64,
    mut fr: f64,
    bad: Option<usize>,
    active: bool,
) -> Vec<Run> {
    let mut runs = Vec::new();
    for i in 0..count {
        rss *= 1.0 + rng.unif(-0.03, 0.05);
        p95 *= 1.0 + rng.unif(-0.06, 0.07);
        iph *= 1.0 + rng.unif(-0.05, 0.05);
        fr = (fr + rng.unif(-0.012, 0.015)).max(0.0);

        // One deliberately regressed run per series, so the failure styling is
        // exercised by the default fixture instead of needing a hand edit.
        let (f, p) = if bad == Some(i) {
            (fr + 0.06, p95 * 1.28)
        } else {
            (fr, p95)
        };

        let it = ((iph * hours) as u64).max(1);
        let dk = (it as f64 * 0.55) as u64;
        let sp = it - dk;
        runs.push(Run {
            date: iso_date(start + i as i64 * step_days),
            run_id: 30200000000 + (i as u64) * 7331 + if kind == "weekend" { 0 } else { 991 },
            sha: rng.sha(),
            kind,
            target_ref,
            duration,
            iterations: it,
            failure_rate: f,
            iters_per_hour: iph,
            rss_mb: rss,
            final_p95: p,
            docker: (dk, (dk as f64 * f) as u64),
            subprocess: (sp, (sp as f64 * f * 1.3) as u64),
            active,
        });
    }
    runs
}

fn write(dir: &Path, name: &str, body: &str) -> std::io::Result<()> {
    fs::write(dir.join(name), body)
}

fn seed_sample(data: &Path) -> std::io::Result<()> {
    let mut rng = Rng(7);
    let weekend = series(
        &mut rng,
        days_from_civil(2026, 6, 1),
        9,
        7,
        "weekend",
        "master",
        216000,
        60.0,
        1180.0,
        2400.0,
        3.1,
        0.02,
        Some(7),
        true,
    );
    let daily = series(
        &mut rng,
        days_from_civil(2026, 7, 6),
        16,
        1,
        "daily",
        "dev",
        79200,
        22.0,
        1120.0,
        2250.0,
        3.4,
        0.015,
        Some(13),
        false,
    );

    let arr = |runs: &[Run]| {
        format!(
            "[\n {}\n]\n",
            runs.iter()
                .map(Run::to_json)
                .collect::<Vec<_>>()
                .join(",\n ")
        )
    };
    write(data, "history.json", &arr(&weekend))?;
    write(data, "history-daily.json", &arr(&daily))?;
    write(data, "latest-summary.json", &weekend[weekend.len() - 1].to_json())?;
    write(data, "latest-summary-daily.json", &daily[daily.len() - 1].to_json())?;
    write(
        data,
        "latest-verdict.json",
        &format!(
            r#"{{"verdict": "pass", "run_id": "{}", "generated_at": "{}T07:30:00Z", "failures": [], "warnings": []}}"#,
            weekend[weekend.len() - 1].run_id,
            weekend[weekend.len() - 1].date
        ),
    )?;
    write(
        data,
        "latest-verdict-daily.json",
        &format!(
            concat!(
                r#"{{"verdict": "regress", "run_id": "{}", "generated_at": "{}T07:30:00Z", "#,
                r#""failures": ["failure rate 8.1% > baseline 2.0% +5pts", "#,
                r#""finalization p95 3180ms > baseline 2310ms +20%"], "warnings": []}}"#
            ),
            daily[daily.len() - 1].run_id,
            daily[daily.len() - 1].date
        ),
    )?;
    write(
        data,
        "latest-report.md",
        "# Sample weekend soak report\n\nSynthetic local preview data.\n",
    )?;
    write(
        data,
        "latest-report-daily.md",
        "# Sample daily soak report\n\nSynthetic local preview data.\n",
    )?;
    println!("  {} weekend runs, {} daily runs", weekend.len(), daily.len());
    Ok(())
}

fn seed_empty(data: &Path) -> std::io::Result<()> {
    write(data, "history.json", "[]")?;
    write(data, "history-daily.json", "[]")?;
    println!("  empty series (bootstrap state)");
    Ok(())
}

// ---------------------------------------------------------------- server

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("md") => "text/markdown; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn respond(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn handle(mut stream: TcpStream, root: PathBuf) {
    let mut line = String::new();
    if BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    })
    .read_line(&mut line)
    .is_err()
    {
        return;
    }

    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    if method != "GET" && method != "HEAD" {
        respond(&mut stream, "405 Method Not Allowed", "text/plain", b"GET only\n");
        return;
    }

    let path = target.split(['?', '#']).next().unwrap_or("/");
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };

    // Local-only, but a preview server still has no business reading outside
    // its own tree: reject traversal rather than relying on the browser not to
    // ask. Only plain, non-empty components are allowed through.
    if !rel
        .split('/')
        .all(|c| !c.is_empty() && c != "." && c != ".." && !c.contains('\\'))
    {
        respond(&mut stream, "400 Bad Request", "text/plain", b"bad path\n");
        return;
    }

    let full = root.join(rel);
    match fs::read(&full) {
        Ok(body) => {
            println!("  200 {rel}");
            respond(&mut stream, "200 OK", content_type(&full), &body);
        }
        Err(_) => {
            println!("  404 {rel}");
            respond(&mut stream, "404 Not Found", "text/plain", b"not found\n");
        }
    }
}

// ---------------------------------------------------------------- entry

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut dir = String::new();
    let mut port = 8770u16;
    let mut seed = "none".to_string();

    let mut i = 1;
    while i < args.len() {
        let need = |i: usize| -> String {
            args.get(i + 1)
                .unwrap_or_else(|| {
                    eprintln!("{} needs a value", args[i]);
                    std::process::exit(2)
                })
                .clone()
        };
        match args[i].as_str() {
            "--dir" => {
                dir = need(i);
                i += 1;
            }
            "--port" => {
                port = need(i).parse().unwrap_or_else(|_| {
                    eprintln!("--port must be a number");
                    std::process::exit(2)
                });
                i += 1;
            }
            "--seed" => {
                seed = need(i);
                i += 1;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }

    if dir.is_empty() {
        eprintln!("--dir is required");
        std::process::exit(2);
    }
    let root = PathBuf::from(&dir);
    let data = root.join("data");

    let seeded = match seed.as_str() {
        "sample" => seed_sample(&data),
        "empty" => seed_empty(&data),
        "none" => Ok(()),
        other => {
            eprintln!("unknown --seed mode: {other}");
            std::process::exit(2);
        }
    };
    if let Err(e) = seeded {
        eprintln!("could not write fixtures to {}: {e}", data.display());
        std::process::exit(1);
    }

    if port == 0 {
        return; // seed-only
    }

    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| {
        eprintln!("could not bind 127.0.0.1:{port}: {e}");
        std::process::exit(1);
    });
    println!("\n  Dashboard:  http://127.0.0.1:{port}");
    println!("  Source:     .github/dashboard/index.html  (re-run to pick up edits)");
    println!("  Ctrl-C to stop and clean up.\n");

    for stream in listener.incoming().flatten() {
        let root = root.clone();
        thread::spawn(move || handle(stream, root));
    }
}
