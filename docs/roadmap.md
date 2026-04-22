# Forge Roadmap

## Multi-file Repositories
The core architecture only manages one file right now. Expanding this to accept `.zip` or `.tar` uploads for context injection is a high priority.

## Helper CLI
A companion CLI (e.g. `forge push .`) to streamline the curl/git workflow. This would wrap the zip, upload, and clone steps.

## Admin Tooling
A management API bound to a different port to query current memory usage and active sessions without exposing it to the public `/forge` endpoint.

## Offline Mode
Fully implementing the `CandleBackend` allowing Forge to be airgapped.

## Richer Diff Modes
Allow Forge to create PRs to upstream GitHub/GitLab instead of purely generating an ephemeral clone.
