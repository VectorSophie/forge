# Forge AGENTS.md

## Product Intent
Forge transforms single-file AI code completions into a Git-native diffing experience. It operates entirely on `curl` and `git clone`.

## Critical Product Invariant
The user must be able to mentally describe Forge like this:
“I curl a file to forge.ai.com, then git clone the generated repo, and the repo shows me both my original file and the AI-completed version as Git history.”
**Do not lose that.**

## Architecture Boundaries
1. **No Web App / UI**: Forge is CLI only.
2. **No Databases**: State strictly resides in `Arc<DashMap>`.
3. **Session Purge on Clone**: The repository is removed from RAM immediately upon the first successful `git clone`.

## Smart HTTP Invariants
Future agents modifying `src/git/smart_http.rs`:
- Do not fake broad Git capabilities.
- Support *only* `git-upload-pack`.
- Deliver the raw packfile constructed in memory.

## Adding Features
If adding a new inference provider, strictly adhere to the `InferenceBackend` trait. If adding multi-file support, adjust `RepoBuilder` to emit multiple Blob and Tree entries, but maintain the strictly two-commit structure (`commit 1 = upload`, `commit 2 = AI completion`).
