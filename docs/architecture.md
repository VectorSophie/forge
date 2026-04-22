# Forge Architecture

## Core Flow
1. **Upload:** Client POSTs a multipart file to `/forge`.
2. **Inference:** Forge detects the programming language, wraps the file contents in a specialized prompt, and queries the Inference Backend (e.g., OpenRouter).
3. **Repo Build:** An entirely in-memory bare Git repository packfile is constructed:
   - A Blob and Tree for the original skeleton file.
   - Commit 1 (Upload) pointing to the first Tree.
   - A Blob and Tree for the AI generated code.
   - Commit 2 (Forge) pointing to the second Tree, using Commit 1 as its parent.
4. **Clone Flow:** The client uses `git clone` with standard Basic Auth over Smart HTTP.
   - `/info/refs?service=git-upload-pack` advertises the main branch and its HEAD.
   - `/git-upload-pack` delivers the pre-computed in-memory packfile.

## In-Memory Session Lifecycle
To prevent unbounded RAM growth, Forge keeps repositories only in memory with a strict lifecycle constraint:
- Sessions have a hard Time-to-Live (TTL) of 1 hour.
- A background clean-up task periodically removes expired sessions.
- **Immediate Invalidation:** To enforce ephemeral properties and security, a session is permanently removed from the map *immediately* after the packfile is served to the first successful `git clone`.

## Smart HTTP Boundaries
Forge implements *just enough* of the Git Smart HTTP protocol to support `clone` and `fetch`. It does not support pushing, parsing incoming packs, or complex delta negotations. Since the repo is deleted upon first clone, complex sync mechanics are unnecessary.
