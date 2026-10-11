//! The keystroke benchmark (DESIGN.md, "Performance budget"): 100k
//! synthetic candidates, a full frecency table, and the searches a person
//! makes while typing — each prefix of several queries, one character at a
//! time, then backspacing, then clearing the query.
//!
//! ```text
//! cargo run --release -p flint-index --example bench            # report
//! cargo run --release -p flint-index --example bench -- --gate  # CI
//! ```
//!
//! With `--gate` it exits non-zero when the 95th percentile is more than
//! 50% over the 16 ms budget, which is how CI catches a slowdown. The
//! margin absorbs CI machines being slower and noisier than a desktop.
//! Run in release: a debug build is several times slower and says nothing
//! about the budget.

use std::time::{Duration, Instant};

use flint_index::frecency::Entry;
use flint_index::{Candidate, Frecency, Index, Search};

const CANDIDATES: usize = 100_000;
const BUDGET: Duration = Duration::from_millis(16);
const GATE: Duration = Duration::from_millis(24);
const NOW: u64 = 1_800_000_000;

/// A small deterministic generator, so every run sees the same data.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a>(&mut self, words: &[&'a str]) -> &'a str {
        words[(self.next() % words.len() as u64) as usize]
    }
}

const WORDS: [&str; 24] = [
    "telemetry",
    "export",
    "frame",
    "window",
    "settings",
    "project",
    "fenix",
    "flint",
    "client",
    "server",
    "packet",
    "decoder",
    "notebook",
    "agenda",
    "release",
    "build",
    "config",
    "parser",
    "render",
    "socket",
    "keymap",
    "theme",
    "launcher",
    "index",
];
const EXTENSIONS: [&str; 6] = ["rs", "toml", "md", "json", "py", "tcl"];

fn candidates() -> Vec<Candidate> {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    (0..CANDIDATES)
        .map(|i| {
            // Two thirds file-like paths, a third app- or issue-like titles,
            // roughly what Fenix's recent files, apps and Jira would give.
            let title = if i % 3 == 0 {
                format!("{} {} {}", rng.pick(&WORDS), rng.pick(&WORDS), i)
            } else {
                format!(
                    "{}/{}/{}_{}.{}",
                    rng.pick(&WORDS),
                    rng.pick(&WORDS),
                    rng.pick(&WORDS),
                    i,
                    rng.pick(&EXTENSIONS)
                )
            };
            Candidate {
                id: format!("bench:{i}"),
                provider: String::new(),
                secondary: format!("{} {}", rng.pick(&WORDS), rng.pick(&WORDS)),
                title,
                boost: 0.0,
            }
        })
        .collect()
}

fn frecency() -> Frecency {
    let mut rng = Rng(42);
    Frecency::from_entries(
        (0..flint_index::frecency::CAP)
            .map(|_| Entry {
                id: format!("bench:{}", rng.next() % CANDIDATES as u64),
                key: rng.pick(&WORDS)[..2].to_owned(),
                score: (rng.next() % 20) as f64,
                at: NOW - rng.next() % (30 * 24 * 3600),
            })
            .collect(),
    )
}

/// The searches typing these queries produces: every prefix, then
/// backspacing to empty.
fn keystrokes() -> Vec<String> {
    let mut out = Vec::new();
    for query in [
        "telemetry export",
        "fenix/render",
        "flnt",
        "pkt dec 12",
        "config.toml",
        "zzzz",
    ] {
        let chars: Vec<char> = query.chars().collect();
        for n in 1..=chars.len() {
            out.push(chars[..n].iter().collect());
        }
        for n in (0..chars.len()).rev() {
            out.push(chars[..n].iter().collect());
        }
    }
    out
}

fn main() {
    let gate = std::env::args().any(|a| a == "--gate");
    if cfg!(debug_assertions) {
        eprintln!("warning: a debug build; run with --release for meaningful numbers");
    }
    let mut index = Index::new();
    index.set_provider("bench", candidates());
    let frecency = frecency();

    let mut times = Vec::new();
    // Twice: the first pass warms caches the way a running daemon is warm.
    for pass in 0..2 {
        for text in keystrokes() {
            let start = Instant::now();
            let hits = index.search(
                Search {
                    text: &text,
                    scope: None,
                    limit: 100,
                    now: NOW,
                },
                &frecency,
            );
            let elapsed = start.elapsed();
            std::hint::black_box(hits);
            if pass == 1 {
                times.push(elapsed);
            }
        }
    }
    times.sort();
    let at = |p: usize| times[((p * times.len()).div_ceil(100)).max(1) - 1];
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "{} searches over {} candidates: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms (budget {} ms, gate {} ms)",
        times.len(),
        index.len(),
        ms(at(50)),
        ms(at(95)),
        ms(*times.last().unwrap_or(&Duration::ZERO)),
        BUDGET.as_millis(),
        GATE.as_millis(),
    );
    if gate && at(95) > GATE {
        eprintln!("the 95th percentile is over the gate: a keystroke would miss its frame");
        std::process::exit(1);
    }
}
