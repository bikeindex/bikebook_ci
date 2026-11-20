---
name: pr
description: >-
  Create, update or finalize the current branch's pull request: "open/make a PR", "update pr",
  "finalize" or "wrap up this branch", or editing its title or body. Runs `/simplify`, the test,
  documentation and comment audits, `cargo fmt`, clippy and the tests against a bikebook checkout,
  and a merge from the base when it conflicts, before writing the body; then pushes.
allowed-tools: Bash, Read, Edit, Glob, Grep, Skill
---

# Pull request workflow

**Shell state does not persist between commands.** Substitute the real values for `<base>` and `<number>`; never set `BASE=` in one command and read it in the next.

## 1. The branch

**A branch whose name doesn't describe the work gets renamed**, while nothing has left the machine: `git ls-remote --heads origin <branch>` is empty, `git rev-parse --abbrev-ref '@{upstream}'` fails (or is `origin/main`, which `git branch --unset-upstream` clears) and `gh pr view --json number` finds none. Then `git branch -m <describes-the-change>`, a rename that moves no files, and say so in your reply. If any of the three finds something, the name is public: leave it and mention the mismatch.

**Then merge the branch's own remote** (`merge-conflicts`) if `git log HEAD..origin/<branch>` lists anything: another session may have pushed to it.

**The base is the open PR this branch builds on, else a branch the user names, else `main`.** Check `gh pr list --state open --json headRefName,number` against `git log --oneline origin/main..HEAD`: a head branch whose commits are in that log is the base, and the body says `Builds on #<n>`. An existing PR keeps its base: `gh pr edit` without `--base`.

## 2. Clean it up

Everything here diffs `origin/<base>...HEAD`, so commit first or also read `git diff HEAD`.

- **`/simplify`**, unless the diff has no code (docs or skills only). A workflow's `run:` block is code. Read `git diff` as soon as its agents return: one that stops mid-check leaves its edit in the tree.
- **Tests the branch adds.** Of each, ask AGENTS.md's question: what bug does this fail on? Delete the ones that only restate the code. A new check over the catalog needs its test that breaks a sample and sees the check report it.
- **Documentation the branch adds** (`AGENTS.md`, `README.md`, `.claude/skills/`): check each claim against the code, and grep for the subject first, since a second statement of a fact is the one that drifts.
- **Comments the branch adds**, against AGENTS.md § Comments, only on lines it changed:

  ```bash
  git diff origin/<base>...HEAD -U0 -- '*.rs' '*.yml' '*.toml' 'bin/*' |
    grep -E '^(\+\+\+ |\+.*(#|//))'
  ```

  An empty result on a diff with code means the pathspec missed it.
- **A pass over the changed files against `AGENTS.md`.** In particular: a change to a file's shape accepts both the old and the new, a change to what's published is a new format (`format-update`), and a version constraint has its `.github/dependabot.yml` ignore.
- **Then, last:** `cargo fmt`, `cargo clippy --all-targets --locked -- -D warnings`, and `BIKEBOOK_DATA=<a bikebook checkout at main> cargo test --locked --no-fail-fast`. bikebook's CI runs this repo's `main`, so a check that fails on bikebook's `main` turns bikebook red the moment it merges: that's the next work, or, if the data is what's wrong, a fix in bikebook first. Ask the user for a checkout's path if you don't know one.

Commit what this produces before going on.

## 3. Merge the base if it conflicts

`git fetch -q origin <base>`, then merge `origin/<base>` through `merge-conflicts` only when the branch conflicts with it (`gh pr view --json mergeable` reads `CONFLICTING`, or with no PR `git merge-tree --write-tree origin/<base> HEAD` exits non-zero) or needs code it just landed.

Then gather, in parallel: `git status`, `git diff origin/<base>...HEAD --stat`, `git log origin/<base>..HEAD --oneline`, `gh pr view --json number,url,title,state,body`. Only an `OPEN` PR counts as existing; for a merged or closed one, create a new PR. No commits ahead of the base: stop and tell the user.

## 4. The body

Write it to a file in the scratchpad directory, never a fixed name in `/tmp`. Match the last few merged PRs (`gh pr list --state merged --limit 5 --json title,body`): a paragraph saying what changes and why, then `##` sections only where a reader needs them. Keep the title under about 70 characters.

- **Describe the end state**, not how it was built. An update rewrites the body rather than appending to it.
- **Say what it does to bikebook**: whether bikebook's data passes the change (it should, unless a data PR has to land first), and what the next publish uploads, if the change reaches `generate_catalog` or `publish.yml`.
- **Steps for the user go in the body**, as numbered steps with exact names: a secret or variable to add, a setting in Cloudflare or GitHub.
- **No "Test plan"**, and nothing CI already runs. Mention a test only when what it verifies is the point.
- **Name other branches by PR number** (`#9`). **Close the issue** the branch, a commit or the user names: `Closes #3` on its own line, or `Part of #3` for one of several tasks. Don't invent a number.
- **No Claude Code attribution footer** (AGENTS.md).

Read the current body before replacing it: carry over anything a human added, or ask about it.

Push with `git push -u origin HEAD`. Then `gh pr edit <number> --title "…" --body-file <file>`, or `gh pr create --draft --base <base> --title "…" --body-file <file>`. Draft unless the user asks for ready.

**Read the body back against the diff**: pick a few names it mentions and confirm each is in `git diff origin/<base>...HEAD`.

## 5. What this run taught you

Did a skill or `AGENTS.md` mislead you, did you work around something undocumented, or did the branch set a convention? Fix it in the skill or `AGENTS.md`, commit, push. Before editing, check the guidance wasn't already there and skipped, and that it pays for itself: how often it fires against what it saves. Most runs find nothing; say so.

Then return the PR URL. With the user's yes to auto-merging, wait on the checks (`gh pr checks <number> --watch`, in the background) and merge the way `merge-pr` says.
