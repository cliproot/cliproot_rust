//! `cliproot wiki lint` — CLI wrapper around [`knowledge::lint::run_lint`].
//!
//! Default run executes structural checks 1–7 + the doctor coverage pass
//! (#8).  `--structural-only` drops #8; `--contradictions` adds the LLM
//! pairwise pass (#9).  Exit code 1 when any broken citation (#2) is found,
//! or any other check fails under `--strict`.

use std::path::PathBuf;

use cliproot_store::Repository;

use crate::knowledge::{lint, llm};
use crate::OutputFormat;

pub fn run(
    cliproot_dir_override: Option<PathBuf>,
    structural_only: bool,
    contradictions: bool,
    strict: bool,
    write_report: bool,
    format: &OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let cliproot_dir = match cliproot_dir_override {
        Some(p) => p,
        None => {
            let cwd = std::env::current_dir()?;
            discover_cliproot_dir(&cwd).ok_or("no .cliproot/ directory found in any ancestor")?
        }
    };
    let project_root = cliproot_dir
        .parent()
        .ok_or("cannot determine project root from .cliproot/")?;
    let repo = Repository::open(project_root)?;

    let opts = lint::LintOpts {
        structural_only,
        contradictions,
        write_report,
    };

    let report = lint::run_lint(&cliproot_dir, &repo, opts, &|s, u, m, t| {
        llm::call(s, u, m, t)
    })?;

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        _ => {
            print_human(&report);
        }
    }

    let has_broken_citation = report
        .checks
        .iter()
        .any(|c| c.id == lint::CheckId::CitationsBroken && !c.findings.is_empty());
    let any_failure = report.checks.iter().any(|c| !c.findings.is_empty());

    if has_broken_citation || (strict && any_failure) {
        std::process::exit(1);
    }
    Ok(())
}

fn print_human(report: &lint::LintReport) {
    println!("Wiki lint — {}", report.generated_at);
    println!("==================================");
    for c in &report.checks {
        let label = if c.findings.is_empty() {
            "OK".to_string()
        } else {
            format!("{} finding(s)", c.findings.len())
        };
        println!("  #{:<2} {:<22} {label}", c.id.number(), c.id.as_str());
        for f in &c.findings {
            println!("      - {f}");
        }
    }
    if let Some(path) = &report.report_path {
        println!("\nReport written to: {}", path.display());
    }
}

fn discover_cliproot_dir(cwd: &std::path::Path) -> Option<PathBuf> {
    let mut dir = cwd.to_path_buf();
    loop {
        let candidate = dir.join(".cliproot");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}
