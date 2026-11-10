---
name: bump-version
description: Bump the project version. Uses the bump-version.sh script (which updates manifest.json, package.json, package-lock.json, every workspace Cargo.toml and Cargo.lock), commits the bump with jj, and tags the commit with git as vX.Y.Z. Trigger when the user asks to "bump version", "release X.Y.Z", or similar.
---

# Bump version

This repo's version lives in:
- `extension/manifest.json`
- `extension/package.json` and `extension/package-lock.json`
- The `Cargo.toml` of every workspace member (crates, server, app/*, xtask; the script asks `cargo metadata`, so new crates are covered automatically)
- `Cargo.lock`, refreshed by the script so the bump commit is complete

Do **not** edit those files by hand. The repo ships `./scripts/bump-version.sh` which keeps them all in sync.

## Steps

1. **Run the script** from the repo root:
   ```bash
   ./scripts/bump-version.sh <X.Y.Z>
   ```
   It validates semver and updates every file.

2. **Review the diff**:
   ```bash
   jj diff --stat
   ```

3. **Commit with jj** — the commit message is just the bare version, matching the existing convention (`0.4.7`, `0.4.6`, …):
   ```bash
   jj commit -m "<X.Y.Z>"
   ```

4. **Tag the commit** with `v<X.Y.Z>`. jj has no native tags, so tag the underlying git commit. The bump commit is now the parent of `@`:
   ```bash
   git tag "v<X.Y.Z>" "$(jj log -r '@-' --no-graph -T 'commit_id' --no-pager)"
   ```
   Verify with `git tag | tail`.

5. Do **not** push the tag automatically. Tell the user the tag was created locally and let them decide when to push.

## Notes

- Only use this skill for version bumps. Regular feature/fix commits go through the normal `commit-with-jj` flow.
- The script uses `jq` and `sed`; both are expected to be installed.
- If the script's printed hint says `git add -p && git commit …`, **ignore it** — this repo uses jj, not git, for commits. The hint is stale.
