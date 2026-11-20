---
name: merge-conflicts
description: >-
  How to bring a branch up to date and resolve git merge conflicts the way this repo expects:
  merge (never rebase or force-push), read each side's intent before choosing, ask when a
  resolution isn't clear-cut, keep the merge commit to just the merge, and audit what merged
  cleanly. Trigger before any `git merge`/`git pull`, on "update from main", "merge main in",
  "this branch is behind", "fix the conflicts", or to pick up another session's push to the
  branch.
allowed-tools: Bash, Read, Edit, Write, Glob, Grep
---

# Merging

1. **Re-read `git branch --show-current`**: another session sharing `.git` may have moved it. The base is the branch the user named, else `gh pr view --json baseRefName`, else `main`. A base whose PR already squash-merged, or part of this branch that already shipped as its own PR: read `references/squash-merged-base.md` first.
2. **Merge from the base only when the PR conflicts with it, needs code it just landed, or the user asks**, never just because it moved. Never rebase, force-push, `--ff-only` or `git pull`. Commit uncommitted work first, then:

   ```bash
   git fetch origin <base>
   git -c merge.conflictStyle=zdiff3 merge --no-edit origin/<base>
   ```

   (`origin/<branch>` instead picks up another session's push.) Report what came in from `git log --oneline HEAD^1..HEAD^2`, not an earlier ahead/behind count.
3. **Resolve each conflict**, reading both sides' intent first: `git log --oneline --merge -- <file>` lists the commits on each side that touched it, and the hunks show branch / `|||||||` base / incoming.
   - The answer is often a combination. Don't drop the branch's purpose, or revert something the base did on purpose.
   - **A value both sides changed takes the base's; the branch only adds what the base lacks.** Ask when it's logic that must be combined, a check's condition say: a wrong silent resolution there is worse than a question.
   - Edit out the unwanted half of each block. `git checkout --ours/--theirs` takes the whole file and loses the other side's clean hunks (`git checkout -m -- <file>` restores the markers).
   - **`Cargo.lock`**: take the base's (`git checkout --theirs Cargo.lock`), resolve `Cargo.toml`, then `cargo check` to add back what the branch's dependencies need. That's part of the resolution.
4. **Audit what merged cleanly.** This repo squash-merges, so a squash-merged base shares no history with its commits, and git silently keeps your side of lines the base deleted. Check `git diff origin/<base> --stat` lists only the branch's own work, and that nothing the base removed came back. Never reset a file the branch changed to the base's version just because the base changed it too.
5. **Commit the merge alone**: `git add` the resolved paths, then `git commit -m "Merge <ref> into <branch>"`, not `--no-edit`, which keeps git's `# Conflicts:` lines in the message. No formatting fixes or call-site fixes for an API the base changed in the merge commit, even where the merge makes them due: commit them after it.
6. **Then** `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings` and the tests against a bikebook checkout, fixing what they find in commits after the merge.

A refused command means re-reading this skill for the command it gives, never asking the user how to merge: a question is only ever which side's intent wins. If its own command is refused, `git merge --abort` so the checkout is left clean, then stop and report.

Pulling part of another branch in with `git checkout <branch> -- <file>` isn't a merge, and overwrites what `main` moved since: `references/squash-merged-base.md` covers slicing.
