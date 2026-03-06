# Contribution Guide

We are happy to accept pull requests and issues from any contributors. Please
note that we try to maintain a consistent quality standard. For a quick overview
please find some of the most important points below.

## Quick Overview

* Keep a clean commit history. This means no merge commits, and no long series
  of "fixup" patches (rebase or squash as appropriate). Structure work as a
  series of logically ordered, atomic patches. `git rebase -i` is your friend.
* Changes should only be made via pull request, with review.
* When changes are restricted to a specific area, you are recommended to add a
  tag to the beginning of the first line of the commit message in square
  brackets e.g., "drc: Fix spacing check for wide metals".
* Create pull requests from a fork rather than making new branches in the main
  repository.
* Do not force push.
* If a relevant bug or tracking issue exists, reference it in the pull request
  and commits.

## Code Style

* Rust code should follow standard Rust conventions and pass `cargo fmt --check`
  and `cargo clippy` without warnings.
* PDK configuration files (TOML) should follow the structure of existing PDKs
  in `pdks/`.

## Adding a New PDK

1. Create a TOML file in `pdks/` following the existing format.
2. Add `include_str!` for the new PDK in `src/pdk.rs`.
3. Add a test case in `tests/drc_per_pdk.rs`.
4. Update the Supported PDKs table in `README.md`.

## License

By contributing to this project, you agree that your contributions will be
licensed under the Apache License 2.0.
