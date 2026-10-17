# Security boundary audit (T289)

Code and fixture inspection only. Nothing was compiled or run for this audit, so the
new tests below are unexecuted. There is no browser, CSP or IPC capability model in
this application; every boundary below is a native in-process or local-socket one.

## Findings by boundary

| Boundary | Finding | Evidence |
| --- | --- | --- |
| Local activation | The Unix listener keeps its directory at mode 0700 and its socket at 0600, and a connection is authenticated by comparing the peer effective user id with the profile owner. The Windows listener builds an owner-only security descriptor and authenticates the process token user. Frames are 1 to 4096 bytes, each connection has a one second deadline, clients have a two second deadline, four trials and four concurrent connections. | `src/desktop/native/activation/unix.rs`, `windows.rs`, `src/desktop/profile/activation.rs`, `server.rs`; the unix peer-credential test is in `unix.rs` |
| In-process admission | The application port bounds ordinary commands at 256, keeps reserved coalesced Stop and Pause controls per entity and a global Quit, bounds the result cache at 1024 and unacknowledged events at 64, starts stop deadlines at port receipt and blocks late commits after cancellation. The library service admits at most 256 operations and reserves payload bytes before accepting them. | `src/desktop/application.rs`, `request_cache.rs`, `stream.rs`, `src/desktop/library_service/port.rs`; `tests/desktop/application.rs` |
| Native file and dialog ports | File selections are opaque handles bound to an owner and a purpose, single use, expire after ten minutes and are capped at 256. Source identity is revalidated at consumption, and destinations are staged through an atomic file stage that validates the target before publishing. The library coordinator consumes a handle with the workspace owner and the matching purpose before any library work starts. | `src/desktop/file_handle.rs`, `src/storage/atomic_file.rs`, `src/desktop/coordinator/library.rs`; `tests/desktop/file_handle.rs` |
| Untrusted imports | PGN and CSV imports are streaming and bounded. An invalid game is excluded as a whole, not partly kept. Import commits only through a frozen preview confirmation with a digest. Imported text never becomes a path. | `src/notation/pgn_import.rs`, `src/puzzle/csv_import.rs`, `src/storage/import_session.rs`; `tests/domain/boundaries.rs`, `tests/storage/import_session.rs` |
| Archive paths | The backup archive has a fixed magic and a 256 KiB manifest bound. Manifest text fields reject NUL and oversize values, and path and dependency counts are bounded. Stored portable paths are only informational: the preview reports that destination locations are kept, and no stored path is used as a write target. | `src/storage/backup/archive.rs`, `manifest.rs`, `preview.rs` |
| Installer | The installation intent binds one profile, a distinct helper generation, a package digest and a canonical stable version, and it is recorded in the profile control record. | `src/desktop/profile/installation.rs` |
| Update redirect and signature trust | Not auditable: the source contains no update client, redirect handling or signature verification. User Story 12 is unimplemented, so this remains a release blocker. The manifest scenarios for it are marked pending. | `tests/fixtures/reference-manifest.json` |
| Credential redaction and exclusions | The source contains no credential, token, password or secret handling to redact, and no network client that would carry one. Backup exclusion of credentials cannot be checked because there are none. Re-audit when update or sync features add credentials. | source search recorded in `tests/desktop/security_boundaries.rs` |
| Release test controls | The shipped sources open no TCP or UDP listener, read no environment variable, accept no invalid-certificate switch and gate code on no feature other than the asset-tooling one. Process arguments are parsed only by the native bootstrap. `chess-tools package-verify` additionally rejects development URLs and test listeners in release packages. | `tests/desktop/security_boundaries.rs`, `tools/packaging/verify.rs` |

## New fixtures (unexecuted)

- `tests/desktop/security_boundaries.rs`: four source-scan tests for the absence of
  listeners and environment reads, argument ownership, the single feature gate and the
  owner-only activation modes.
- `tests/domain/boundaries.rs`, `tests/storage/game_document.rs`: malformed and
  truncated FEN, PGN, CSV and stored-document inputs never panic and are refused or
  accepted canonically.

## Outstanding

Update trust, native Windows confinement for the engine coverage reader, real Windows
and macOS runs of the packaging verifier, and a re-run of everything above on a
compiled build.
