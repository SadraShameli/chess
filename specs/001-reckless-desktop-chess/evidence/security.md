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
| Update redirect and credential forwarding | The HTTP client is HTTPS-only, follows no redirect itself and never retries. The redirect loop allows only api.github.com and the two GitHub asset hosts on port 443 without userinfo, caps hops at five, and removes the bearer token on the first cross-origin hop and never restores it. Fixed: the request type derived `Debug` and would have printed the bearer token; it now redacts it. | `src/update/http.rs`, `src/update/discovery.rs`; `tests/update/security.rs` |
| Signature trust | A build with no embedded public key refuses every manifest (fail closed). The manifest is verified (minisign, signature capped at 16 KiB) before parsing, is capped at 1 MiB, and must be stable, canonical, newer than the installed version, equal to the release tag, and list the chosen asset in the release. Asset names, lengths and SHA-256 come only from the signed manifest. Fixed: asset names now also refuse `:` (drive-relative and stream names), trailing dots or spaces and Windows device names, through one shared rule used for both the manifest and the cached receipt. | `src/update/verifier.rs`, `manifest.rs`, `discovery.rs`; `tests/update/security.rs` |
| Cache validation | A download is streamed against the signed length and digest and the partial is deleted on any failure. A completed cache holds one package plus a receipt, and a replacement is validated before the old package is removed. The cached receipt is unsigned, so installing after a restart needs a freshly verified manifest asset; the install step re-hashes the package against that asset before writing an intent. | `src/update/download.rs`, `cache.rs`, `install.rs`; `tests/update/download.rs`, `install.rs` |
| Installer handoff | The committed intent binds profile, helper generation, package digest and version. The helper takes no credential, waits for the parent to exit, takes the profile lease, rechecks the digest against the committed intent and records the outcome. Installers refuse relative package paths. Fixed: the AppImage swap staged through a predictable, symlink-following copy; it now creates an exclusive, randomly named file. | `tools/packaging/update_helper.rs`, `src/desktop/native/update/*.rs`; `tests/update/security.rs`, `helper.rs` |
| Credential redaction and exclusions | The token lives in the operating-system keyring or in memory only. Its `Debug` output is redacted, it has no `Display` or `Serialize`, only the discovery, download and keyring modules read its raw value, and nothing in storage, backup, preferences, workbench export or presentation references it. No update or credential code prints or logs. There is no token entry control yet; when one is added it must use a masked field. | `src/update/credentials.rs`, `src/desktop/native/credentials.rs`; `tests/desktop/security_boundaries.rs`, `tests/update/credentials.rs` |
| Release test controls | The shipped sources open no TCP or UDP listener and accept no invalid-certificate switch. The only environment read is `APPIMAGE`, which selects between two already signed Linux packages. Only the native bootstrap reads process arguments, and the only feature gate is the asset-tooling one. `chess-tools package-verify` rejects development URLs and test listeners in release packages. | `tests/desktop/security_boundaries.rs`, `tools/packaging/verify.rs` |

## Residual risks

- The Debian install runs `pkexec dpkg --install` on a user-writable cached file. The
  helper rechecks the digest first, but a process running as the same user could replace
  the file between that check and dpkg reading it. Closing this needs a privileged helper
  that copies the file into a root-owned directory before hashing.
- The AppImage and macOS bundle swaps replace files in user-writable locations, so they
  provide integrity against network and cache tampering only, not against a hostile
  process of the same user.
- A macOS swap that succeeds but fails to remove its staging directory reports Failed
  although the bundle was replaced.

## New fixtures (unexecuted)

- `tests/desktop/security_boundaries.rs`: four source-scan tests for the absence of
  listeners and environment reads, argument ownership, the single feature gate and the
  owner-only activation modes.
- `tests/update/security.rs`: bearer redaction, bearer drop on cross-origin redirects,
  lookalike hosts, userinfo and ports, unsafe asset names and receipts, the unprovisioned
  verifier, oversize signatures, relative installer packages and the exclusive AppImage
  swap.
- `tests/desktop/security_boundaries.rs` also scans for logging in update code, raw token
  access, credential references outside the transport modules and the helper, and the
  single permitted environment read.
- `tests/domain/boundaries.rs`, `tests/storage/game_document.rs`: malformed and
  truncated FEN, PGN, CSV and stored-document inputs never panic and are refused or
  accepted canonically.

## Outstanding

Native Windows confinement for the engine coverage reader, real Windows
and macOS runs of the packaging verifier, and a re-run of everything above on a
compiled build.
