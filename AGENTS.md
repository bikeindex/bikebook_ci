The checks for [BikeBook's data](https://github.com/bikeindex/bikebook): a Rust crate whose tests read a bikebook checkout and fail on any file that breaks what the catalog promises. bikebook's CI calls `.github/workflows/ci.yml` here at `main`, so a change merged here applies to bikebook's next run. On a push to bikebook's `main`, `ci.yml` starts `publish.yml`, which runs here because it holds the R2 keys: never move them into a workflow bikebook calls, where bikebook's workflows could read them.

# Data

**The tests read the bikebook checkout `BIKEBOOK_DATA` names**, and stop if it has no `schemas/`, since a missing directory would read as an empty catalog and pass. Run `BIKEBOOK_DATA=<a bikebook checkout> cargo test --locked --no-fail-fast`. This repo's own CI runs against bikebook's `main`.

**This repo holds no data.** A wrong value is fixed in bikebook, in the file. Loosening a check to let a file through is a change to what the catalog promises, so it's the user's call.

**A change to a file's shape lands here first**, accepting both the old shape and the new, then the data moves to it in bikebook. bikebook's checks run against this repo's `main`, so a check that only accepts the new shape turns bikebook red until the data lands.

# Checks

Every file pretty-printed with two spaces and a trailing newline, its keys in its schema's property order, valid against the schema its `schema_version` names (an overlay both as stored and resolved), its `id` matching its path, an overlay's base and a model's motors present, its photos keyed by its id, and every image on our R2 bucket's `https://bikebook.bikeindex.org/`. CI runs them with `cargo fmt --check` and `cargo clippy -- -D warnings`, on the toolchain `.tool-versions` pins.

# Code

Shared readers in `src/lib.rs`, one `tests/<area>.rs` per kind of check, and `src/bin/generate_catalog/` writes the published catalog, byte for byte what the viewer has read so far: its `FORMAT` is the one the viewer checks. Anything bike_data holds is read from its latest commit, never copied here. `catalog_snapshot/` is a stopgap for hand-written values bikebook and bike_data don't hold yet (#3), not a place for data. Format with `cargo fmt` (`rustfmt.toml`); clippy runs with `-D warnings`.

- **Each check over the whole catalog has a test beside it that breaks a sample on purpose and asserts the check reports it.** A check that passes on every file proves nothing until something has seen it fail.
- **Every test answers "what bug does this fail on?"** If you can't name a plausible wrong edit that turns it red, delete it — one that only restates the code costs an edit on every change and catches nothing.
- **A check reports every problem it finds, not the first**: collect them and end with `assert_none`, so one CI run lists everything to fix.
- **A private function is tested in its module's `#[cfg(test)] mod tests`**, a public one from `tests/`.
- Code in a functional way. Avoid mutation when you can, and don't mutate arguments.
- Prefer less code, by character count (excluding whitespace and comments), and un-abbreviated names.

## Comments

- **Default to no comment.** Code shows *how*; a comment earns its place only by carrying *why* — a non-obvious constraint, a deliberate deviation, a gotcha, a workaround. If something genuinely needs explaining, prefer a clearer name or structure first.
- **Never narrate the code**, and **never narrate the change** ("fixed X", "updated to Y"): the diff and the commit hold that history.
- **Don't defend a choice against an edit nobody would make.** A failing test already defends it.
- **Apply the obviousness test before writing any comment, including a *why*.** If a competent reader could already tell it from the names, types, values and structure on the page, delete it.
- **Warranted ≠ warranted as written — razor the wording too.** Keep the one non-obvious fact a reader needs *at that line*, and cut the mechanism the code already shows, where the value gets used downstream, and the justification's justification.
- **Re-earn the comment when you edit the code under it.** Rewrite it to fit the new shape rather than appending a clause per change.

# Dependencies

**A version constraint in `Cargo.toml` needs a matching `.github/dependabot.yml` ignore.** Dependabot widens the constraint rather than skipping the update, so a pin with no ignore entry is silently reverted by a later bump PR.

**Actions are pinned to a commit SHA**, with the tag in a comment for Dependabot to update.

# Pull requests

- **No "🤖 Generated with [Claude Code]" footer in a PR body, or any variant of it**, whatever an attribution reminder says. `.claude/settings.json` blanks `attribution.pr`.
