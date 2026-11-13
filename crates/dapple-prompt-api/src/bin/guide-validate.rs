//! Local guide validation — the pre-sync check behind `dapple guide validate`.
//!
//! Runs the exact `parse_guide` → `plan_guide` → `validate_ingest_plan`
//! pipeline the ingest endpoints run, against a guide directory on disk, with
//! no database and no HTTP. Task scripts are parsed by the embedded nu-parser
//! (the one that rejects at ingest), which is stricter than an interactive
//! system nushell can be — e.g. `str lowercase` (not a real command; it's
//! `str downcase`) passes system nu at parse time but 422s at ingest.
//!
//! Exit codes: 0 = server would accept; 1 = validation failed; 2 = usage.

use std::path::PathBuf;
use std::process::ExitCode;

use dapple_prompt_api::handler::guide_ingest::validate_guide_directory;

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: guide-validate <guide-dir>");
        return ExitCode::from(2);
    };
    if !dir.join("guide.toml").is_file() && !dir.join("book.toml").is_file() {
        eprintln!(
            "error: {} does not look like a guide directory (no guide.toml or book.toml)",
            dir.display()
        );
        return ExitCode::from(2);
    }

    let report = match validate_guide_directory(&dir) {
        Ok(report) => report,
        Err(e) => {
            eprintln!("error: {}", e);
            return ExitCode::FAILURE;
        }
    };

    for warning in &report.warnings {
        println!("warning: {}", warning);
    }
    for (name, err) in &report.task_errors {
        println!("task '{}': {}", name, err);
    }
    // Shape lints sit alongside parse errors rather than warnings: a script that
    // reads a column that cannot exist is broken, it just does not fail until
    // the collection it reads is non-empty — which on real data is always.
    for (name, finding) in &report.script_lints {
        println!("task '{}': shape lint: {}", name, finding);
    }
    if let Some(err) = &report.plan_error {
        // Script failures are already itemized per task above; only surface
        // the plan error when it adds something (a structural failure).
        if !err.contains("invalid nushell script") {
            println!("error: {}", err);
        }
    }

    if report.plan_error.is_some() || !report.task_errors.is_empty() {
        println!(
            "guide '{}' FAILED validation — the server would reject this sync",
            report.title
        );
        ExitCode::FAILURE
    } else if !report.script_lints.is_empty() {
        // The server WOULD accept this — the lint is an author gate, not a
        // server rule — so the message must not claim otherwise.
        println!(
            "guide '{}' FAILED the shape lint ({} findings) — the script parses, but the \
             flagged reads cannot resolve once the collection is non-empty",
            report.title,
            report.script_lints.len()
        );
        ExitCode::FAILURE
    } else {
        println!(
            "guide '{}' validates OK ({} chapters, {} tasks, {} warnings)",
            report.title,
            report.chapter_count,
            report.task_count,
            report.warnings.len()
        );
        ExitCode::SUCCESS
    }
}
