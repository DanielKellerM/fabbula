// Copyright 2026 Daniel Keller <daniel.keller.m@gmail.com>
// Licensed under the Apache License, Version 2.0.
// SPDX-License-Identifier: Apache-2.0

//! Commit message linter for CI.
//!
//! Validates commit messages follow Chris Beams conventions:
//! - Subject line <= 100 chars
//! - Subject starts with capital letter
//! - Subject does not end with period
//! - No fixup!/squash! commits
//! - If multi-line: line 2 is blank
//! - No merge commits

use std::process::{Command, ExitCode};

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn commit_exists(sha: &str) -> bool {
    git(&["cat-file", "-e", &format!("{sha}^{{commit}}")]).is_some()
}

fn determine_range() -> String {
    let base_ref = std::env::var("GITHUB_BASE_REF").unwrap_or_default();
    let before = std::env::var("GITHUB_EVENT_BEFORE").unwrap_or_default();
    let null_sha = "0000000000000000000000000000000000000000";

    if !base_ref.is_empty() {
        // Pull request: lint commits in the PR
        return format!("origin/{base_ref}..HEAD");
    }

    if !before.is_empty() && before != null_sha && commit_exists(&before) {
        // Push: lint new commits (only if the before-commit exists locally)
        return format!("{before}..HEAD");
    }

    // Fallback: lint only HEAD
    "HEAD~1..HEAD".to_string()
}

fn lint_commit(sha: &str) -> Vec<String> {
    let mut errors = Vec::new();

    let Some(msg) = git(&["log", "-1", "--format=%B", sha]) else {
        errors.push(format!("ERROR [{sha}]: could not read commit message"));
        return errors;
    };

    let mut lines = msg.lines();
    let subject = lines.next().unwrap_or("");

    // Check for merge commits
    if let Some(parents) = git(&["log", "-1", "--format=%P", sha])
        && parents.split_whitespace().count() > 1
    {
        errors.push(format!("ERROR [{sha}]: merge commits are not allowed"));
        return errors;
    }

    // Subject line length
    if subject.len() > 100 {
        errors.push(format!(
            "ERROR [{sha}]: subject line exceeds 100 chars ({}): {subject}",
            subject.len()
        ));
    }

    // Subject starts with capital letter
    if !subject.starts_with(|c: char| c.is_ascii_uppercase()) {
        errors.push(format!(
            "ERROR [{sha}]: subject must start with a capital letter: {subject}"
        ));
    }

    // Subject does not end with period
    if subject.ends_with('.') {
        errors.push(format!(
            "ERROR [{sha}]: subject must not end with a period: {subject}"
        ));
    }

    // No fixup/squash commits
    if subject.starts_with("fixup!") || subject.starts_with("squash!") {
        errors.push(format!(
            "ERROR [{sha}]: fixup/squash commits must be resolved: {subject}"
        ));
    }

    // If multi-line, line 2 must be blank
    if let Some(line2) = lines.next()
        && !line2.is_empty()
    {
        errors.push(format!(
            "ERROR [{sha}]: line 2 must be blank (separates subject from body): {subject}"
        ));
    }

    errors
}

fn main() -> ExitCode {
    let range = determine_range();

    let shas = git(&["log", "--format=%h", &range]).unwrap_or_default();
    let mut total_errors = 0;

    for sha in shas.lines() {
        if sha.is_empty() {
            continue;
        }
        let errors = lint_commit(sha);
        total_errors += errors.len();
        for e in &errors {
            eprintln!("{e}");
        }
    }

    if total_errors > 0 {
        eprintln!("\nFound {total_errors} commit message error(s)");
        ExitCode::FAILURE
    } else {
        println!("All commit messages pass lint checks");
        ExitCode::SUCCESS
    }
}
