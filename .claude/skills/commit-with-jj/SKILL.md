---
name: commit-with-jj
description: Commit changes in this repo using jj (jujutsu), not git. Covers status, diff, tracking new files, creating a new commit, and amending the current commit with squash. Trigger on any commit request in this repository.
---

# Commit with jj

This repo is managed with **jujutsu (`jj`)** colocated on top of git. Always use `jj` for staging and committing. Do not run `git add`, `git commit`, or `git rebase` for normal work — they bypass jj's working-copy model and cause confusion.

## Workflow

1. **Inspect state** in parallel:
   ```bash
   jj status
   jj diff --stat
   jj log -r 'mutable()' --no-graph -T 'commit_id.short() ++ " " ++ description.first_line() ++ "\n"'
   ```

2. **Track any new files** the user wants in the commit. jj does not auto-track untracked paths — list them explicitly so unrelated files (`BUILD.md`, build artefacts, archives, etc.) stay out:
   ```bash
   jj file track path/to/new1 path/to/new2
   ```

3. **Decide: new commit or amend**
   - **New commit** for a new logical change:
     ```bash
     jj commit -m "<conventional message>"
     ```
     This finalises `@` and opens a fresh empty working copy on top.
   - **Amend the previous commit** when the new edits belong to the most recent commit (typo, missed file, review feedback):
     ```bash
     jj squash
     ```
     `jj squash` (no args) folds the current working-copy changes into `@-`. Use this instead of `git commit --amend`.

4. **Verify** with `jj log -r '@-' --no-graph -T 'description ++ "\n"'`.

## Commit message style

The repo follows conventional commits — recent examples from `jj log`:

```
refactor(srs): JMdict as single source of truth for card display
feat(storage): mirror SRS deck to storage.local + JSON export/import
fix(review-tab): move card progress to subtle top-right corner
perf(options): debounce _yomeru_db_v storage listeners
```

Use a body when the *why* isn't obvious. Do **not** add a `Co-Authored-By` trailer for jj commits unless the user asks.

## Don'ts

- Don't `git commit`, `git add`, `git stash`, `git rebase -i`. Use jj equivalents.
- Don't `jj describe` and then forget to advance `@` — `jj commit -m` does both in one step.
- Don't `jj file track` paths the user clearly hasn't asked to commit (random untracked files at the repo root, downloaded archives, etc.).
- Don't push tags or branches without an explicit user request.

## Version bumps

Version-bump commits have their own dedicated flow — see the `bump-version` skill. Don't hand-edit `manifest.json` / `package.json` / `Cargo.toml` versions here.
