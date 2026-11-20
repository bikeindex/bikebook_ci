---
name: merge-pr
description: >-
  Deciding which open PRs are ready to merge (checks, mergeability, nobody else on it, a
  description matching the diff), merging them oldest-first unless main's latest finished CI run
  is red, updating conflicting PRs one at a time, and handing each merged PR's follow-ups to a
  fresh session. Use when asked which PRs are ready, to merge one or a batch, to "merge
  everything that's green", to run a merge loop, or to update open PRs from main, even just
  "check the PRs". Merges only with the user's auto-merge yes, commenting "Agent merging this PR"
  first.
---

# Merging PRs

**Ask the user before the first merge in a session**, as soon as you know one may come, so the answer is in before the checks go green: "Merge PRs automatically once CI is green and nothing needs you?" One yes covers the rest of the session; a scheduled task carries it in its prompt. Without one, stop at the ready list and hand it over. If a merge is refused, stop merging: don't look for another route to it.

A merge here changes bikebook's next CI run, which calls this repo's `main`, and a merge touching `generate_catalog` or `publish.yml` changes bikebook's next publish.

The repo squash-merges and deletes the head branch. Comment first, so the PR records that an agent merged it: `gh pr comment <n> --body "Agent merging this PR"`, then `gh pr merge <n> --squash`. If the merge fails, reply on the PR with why and leave it.

## Before any merge: main's CI

`gh run list --workflow CI --branch main --status completed --limit 1`. Green: merge. Never wait for a run in progress. Red: read it (`gh run view <id> --log-failed`) and merge nothing until it's understood. A check failing on bikebook data that changed since is bikebook's fix, not a reason to hold this repo's PRs; say so to the user.

## Is this PR ready?

A PR that fails any of these waits.

1. **Not a draft**, and **its checks green**: none failing, none running.
2. **Nobody is working on it.** Treat it as in progress when a worktree has its branch checked out and locked (`git worktree list --porcelain`; `.claude/worktrees/bridge-cse_<id>` is the open session `session_<id>`), has uncommitted changes, or is named in another checkout's `.claude/taken_over_worktree`; when it was pushed to in the last 30 minutes (`gh api "repos/{owner}/{repo}/activity?ref=<branch>&per_page=1" --jq '.[0] | .timestamp, .after'`; a commit's own date isn't when it was pushed); or when one of your own agents is still on it.
3. **`mergeable: MERGEABLE` and `mergeStateStatus: CLEAN`.** After `main` moves, GitHub reports `UNKNOWN` for a while: poll up to a minute, then test it yourself with `git merge-tree --write-tree origin/main origin/<branch>`.
4. **The description matches the diff.** Read the body and the comments:
   - "Don't merge", WIP, an unchecked list, or a step it gives the user (a secret, a Cloudflare setting) that hasn't been confirmed done.
   - "Builds on #N" while #N is open: see Stacked PRs.
   - Stale references: "left for #12" when #12 merged.
   - **Behaviour the diff changes that the body doesn't mention.** The squash commit becomes `main`'s only record of it.
5. **Nothing on it needs the user**: no unanswered question in the body or comments, no unresolved review thread.
6. **Not on the user's skip list** ("leave #11"). Re-read it every round.
7. **Not the user's to merge**, however green. Say it's ready and leave it:
   - a PR that loosens a check, which changes what the catalog promises (AGENTS.md);
   - `format-update`'s second PR, dropping a format;
   - a PR whose body says it only merges once something else has landed (a bikebook data PR, a reader's release).

Fixing a description goes through the `pr` skill: re-fetch the body right before editing and change only the stale lines.

## Merging a batch

Oldest first. Re-check mergeability right before each merge: every squash changes `main`. Skip a PR that conflicts now rather than forcing it.

### Stacked PRs

A branch cut from another PR's branch carries that PR's commits. Its body usually says so; otherwise a diff against `main` far larger than the body accounts for (`git diff --stat origin/main...origin/<branch>`) gives it away. Merge from the bottom. Then, before merging the next one up, merge the lower PR's final commit (`gh pr view <lower> --json headRefOid`) into it, before `origin/main` (`merge-conflicts`' `references/squash-merged-base.md`), re-read its body against its now smaller diff, and wait for its checks. If its base on GitHub is still the deleted lower branch, `gh pr edit <n> --base main`.

## Bringing a conflicting PR up to date

One at a time, oldest first, since each update is against the `main` the previous merge left. Use the branch's worktree if it's unlocked and clean, otherwise add one under `.claude/worktrees/` (`git worktree add .claude/worktrees/<name> <branch>`). Never change another checkout's branch. Follow `merge-conflicts`, push with `git push origin HEAD`, and check the description still matches before the PR comes up for merge.

A subagent can do an update: give it the worktree path and branch ("work only there"), the PR's purpose in one line, the recently merged PRs touching the same files, "follow `merge-conflicts`; stop if a resolution isn't clear-cut; no amend or force-push", and ask for each conflict's resolution, the pushed SHA, and whether the body still matches. An approval the user gave you isn't consent to a subagent: do that part yourself. Check what an agent reports against git before repeating it.

## After a merge: follow-ups

1. **Gather** what the merged PR left undone: its body's "not done" and "left for later" lines, its comments and unresolved threads, the reports of agents that worked on it. Work waiting on another open PR stays with that PR.
2. **Hand the user what's theirs** as a question in your reply: a loosened check, a setting only they can change.
3. **Nothing left:** say `Nothing to follow up on from #<n>`.
4. **Something to do:** start a fresh session for it rather than doing it here, where every call re-reads the whole merge run. `create_session` with a prompt opening "`merge-pr` follow-up to #<n>: start a new branch off main named <name>" and carrying what only this conversation knows: each follow-up, where it stands, and the choices already made. Reply with what you handed over and the session's id. Without `create_session`, give the user that prompt. After a batch, one session takes every merged PR's follow-ups.

## After a batch

Remove each worktree this round added once its work is pushed (`git worktree remove <path>`), then invoke `remove-archived-worktrees` for the sessions the batch finished.

## Running it as a loop

A `/loop` dies with its session, so it only suits a loop the user is watching. One that runs unattended is a scheduled task (the `schedule` skill) starting a fresh session each firing, its prompt carrying the whole round: these checks, main's CI first, oldest first, one update at a time, the skip list and the auto-merge yes. When the skip list changes, update the prompt.

## Reporting

Each round: what merged and its follow-up, what was updated (with the pushed SHA), and what's held and why: in progress elsewhere, conflicting, the description doesn't match, or waiting on the user. Say plainly when you held something the user might expect merged.
