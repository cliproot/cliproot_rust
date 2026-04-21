//! `cliproot wiki query` — CLI wrapper around [`knowledge::query::run_query`].
//!
//! Two-phase retrieval over the compiled wiki.  Phase 1 extracts keywords
//! from the prompt (cheap Haiku call) and picks candidate articles via
//! `index::select_articles_for_compile`.  Phase 2 reads the selected bodies
//! and produces an answer with `[cliproot:sha256-...]` citations.

use std::path::PathBuf;

use cliproot_store::Repository;

use crate::knowledge::{llm, query};
use crate::OutputFormat;

pub fn run(
    prompt: &str,
    cliproot_dir_override: Option<PathBuf>,
    file_back: bool,
    top_k: usize,
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

    let opts = query::QueryOpts { file_back, top_k };

    let outcome = query::run_query(prompt, &cliproot_dir, &repo, opts, &|s, u, m, t| {
        llm::call(s, u, m, t)
    });

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&outcome)?);
        }
        _ => print_human(&outcome),
    }

    match outcome {
        query::QueryOutcome::Error(e) => Err(e.into()),
        _ => Ok(()),
    }
}

fn print_human(outcome: &query::QueryOutcome) {
    match outcome {
        query::QueryOutcome::Answer {
            text,
            cited_clips,
            consulted_articles,
            qa_path,
        } => {
            println!("{text}\n");
            println!("─────");
            if !consulted_articles.is_empty() {
                println!("consulted: {}", consulted_articles.join(", "));
            }
            if !cited_clips.is_empty() {
                println!("citations: {} clip(s)", cited_clips.len());
            }
            if let Some(p) = qa_path {
                println!("persisted: {}", p.display());
            }
        }
        query::QueryOutcome::BudgetExceeded(r) => {
            eprintln!("cliproot wiki query: BUDGET_EXCEEDED {r}")
        }
        query::QueryOutcome::Skipped(r) => eprintln!("cliproot wiki query: SKIPPED {r}"),
        query::QueryOutcome::Error(e) => eprintln!("cliproot wiki query: ERROR {e}"),
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
