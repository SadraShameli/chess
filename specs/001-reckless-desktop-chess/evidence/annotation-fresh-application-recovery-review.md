# Annotation recovery through fresh application requests

## Status and scope

Three tests are authored and unrun. They require the integrated parent working
tree at `/Users/sadrashameli/Personal/chess`. This isolated checkout and the
parent reported HEAD `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68` when inspected.
The integrated application routing and reusable fixtures are parent working-tree
dependencies absent from this checkout's HEAD. They were inspected read-only,
without copying or applying them here.

Only these new files belong to this handoff:

- `tests/desktop/library_service/notes_application_recovery_review.rs`
- `tests/desktop/library_service/notes_application_recovery_review_fixture.rs`
- This evidence document.

The global guidance, project constitution, catalog, and all seven canonical rules
were read. The parent and isolated copies of the seven rules have identical
SHA-256 hashes. User-level path rules do not match the Rust or Markdown files.
No Spec Kit workflow was invoked.

## Cases and expected baseline failures

`retained_notes_failure_emits_retry_save_bound_to_a_fresh_actual_application_request`
starts from an actual application Open publication, edits the presentation draft,
and correlates its original Save Notes intent to a real admitted Library action.
A SQLite trigger aborts the accepted edit's save. The panel observes the original
actual application failure and its exact certified source binding. A new real
application request is prepared for Editor RetrySave, and its ID is passed to
`PresentationAnalysisAnnotations::retry_save`.

The expected baseline assertion is the retry intent's request ID: current
presentation source returns the original SaveNotes intent and ID. Assertions also
require the dedicated RetrySave action, the complete original certified context,
successful admission and attachment of the fresh actual receipt, and presentation
completion from that request's actual Saved result. The known application
RetrySave is submitted independently of the emitted intent so the service seam
can still be observed when presentation correlation fails. Passing requires both
the emitted intent and the independently prepared actual action to agree.

`fresh_notation_application_retry_accepts_the_original_certified_binding_after_sqlite_failure`
isolates the notation service route without presentation. It submits a new actual
application Editor RetrySave using the original error's retained expected context.
Current notation code checks the advanced owner context, so the expected baseline
failure is the assertion requiring Saved from the fresh application before
cleanup. The candidate result is captured before fallback recovery and cannot be
replaced by the cleanup result.

`fresh_played_application_retry_recovers_receiving_publication_pressure_without_replaying_notes`
is the positive control for the existing original-lower-receipt retry mapping.
It reserves positive storage memory pressure after opening the source. It checks
the actual original lower receipt's Publishing context and typed Saturated
publication error, its absence of a storage receipt, and the actual registry
revision. After releasing pressure, a fresh application RetrySave must receive
Saved and reconcile the original service and lower storage receipts. This
control has not been executed; no green result is claimed.

## Real boundaries and cleanup

The new module includes
`open_application_recovery_review_fixture.rs` by path. Its coordinator, actual
application admission, observed real service receipts, registry, bounded driver,
and shutdown implementation are reused from their existing source. The focused
new child adds scenario orchestration and owned observations. Shared seeding,
clock, SQLite execution, memory pressure, and Library cleanup remain on the
existing `pub(crate)` fixture boundary.

All original failures must retain the original actual request ID and the exact
original game, source revision, selected node, storage revision, epoch, and
generation. Resubmitting the original actual application request before and after
recovery must return the same terminal failure object without another provider
dispatch. The new request must have a distinct actual receipt and reconcile the
same actual Saved acknowledgment as its service and the original retained service
operation. Notation receipt identity is checked with `Arc::ptr_eq`; played
receipts are compared to the actual original lower storage receipt.

The SQL trigger targets the temporary database's `library_entries` update and
raises `controlled fresh application notes refusal`. Before retry, the trigger is
removed and the original durable revision, notes, and modified UTC must remain
unchanged. Accepted source revision and generation advance exactly once. Retry
uses UTC 9000 while the only accepted edit used UTC 1000. Final durable modified
UTC must remain 1000, the wall provider must have exactly one call, and source
revision and generation must remain at the original accepted edit after recovery.
The durable storage revision must advance once.

On a failed candidate notation retry, cleanup first removes the trigger, sends a
supported actual application Open, reads the real retained publication context,
then sends an actual application RetrySave using that current context. This is
the known baseline cleanup path. For a failed played candidate, cleanup retains
the original binding required by the existing played mapping. Cleanup outcomes
remain separate from the previously captured candidate outcomes.

The trigger and pressure are released even when scenario observation returns an
error. Scenario receipt owners and initial publications are dropped before the
reused fixture closes the coordinator, service, registry, SQLite actor, and
executor. Behavioral assertions run only after close completes. The returned
cleanup observations require one coordinator shutdown, the entire storage memory
budget available, and the capacity lease released. Initial fixture setup failures
remain setup errors and do not claim a completed cleanup experiment.

No publication, result, receipt, or context is constructed for a simulated
response. Application Library values are read as actual routed service results;
editor publications are never reconstructed from board DTOs. There are no sleeps,
mock service outcomes, ignored cases, or lint suppressions in the new files.

## Parent registration and verification

The parent must mount the new module exactly once in
`tests/desktop/library_service.rs`:

```rust
#[path = "library_service/notes_application_recovery_review.rs"]
mod notes_application_recovery_review;
```

The two fixture modules are included by the new module and need no additional
registration. The existing Open review module's registration remains independent.
The parent owns the Cargo lane and must obtain real semantic reds before repairing
either seam, then rerun the cases and applicable checks after repair. Existing
presentation coverage that expects reuse of the original service receipt must be
reconciled with the new actual application recovery contract by the parent.

Only standalone rustfmt and static source/patch inspection are performed here.
Formatting used the installed `/opt/homebrew/Cellar/rust/1.99.1/bin/rustfmt` with
edition 2024 and child traversal disabled; no toolchain was installed. No Cargo,
build, test, Clippy, runtime, or CPU diagnostic command was run. No agents,
production changes, existing-file edits, commits, or pushes were used.

Static validation passes: formatting, staged whitespace checks, a reverse patch
check in the isolated checkout, and a read-only patch applicability check against
the integrated parent. Source scans found no comments, sleeps, or suppression
attributes in the new Rust files. These checks do not establish compilation or
behavior.

Compilation, baseline failure count, fixed behavior, widget interaction, GPU,
accessibility, native window, platform, and release acceptance are unverified.

## Artifact integrity

The new-file binary patch is exported against HEAD to
`/tmp/claude-annotation-fresh-application-recovery.920pym/notes-application-recovery.patch`.
Its companion `SHA256SUMS` covers the patch, new files, and
`parent-api-dependencies.sha256`. The latter records the actual parent source
hashes used by this handoff. These hashes identify a working-tree API snapshot,
not an independently committed dependency set. The patch intentionally contains
no dependency implementations or registration change.

Authored Rust file SHA-256 values after formatting:

```text
cbe83a95048308e25aecff477a5fa539e4ce7fa159cccfe509ebf172395bd7e3  tests/desktop/library_service/notes_application_recovery_review.rs
3310da68e237f50571a6bdae5674246b28e22edaf626eb6cda2fabfaf8f3e229  tests/desktop/library_service/notes_application_recovery_review_fixture.rs
```

Principal parent API and fixture SHA-256 values:

```text
c65fe112f4d1cc7ee1c2dea601fae4b6646a3e7ce6cbc6cf97fd895338f34752  src/desktop/application/library.rs
ae9f18f22f5b651fc6ec481cc6f8599242ad0bd894428f14a7eff337b50035aa  src/desktop/coordinator/library.rs
6b2095eca1cc7abfa66460e8249d59d325d629ce18fc5a35b902f9362be4b6b1  src/desktop/library_service/notation.rs
8da338a8252e277daf850d88f7a41eeb1b89c00425cd8321d7481e7553cc34b4  src/desktop/library_service/played.rs
c44eec233bd6b32d44e8c3442d4739358fa7358ca1857bc5fe7b600a1a753ad1  src/presentation/analysis/annotations.rs
be39c37210a4343f904468c5c2e6a9f9fea4526f84777ee4bb87f4a83e81b3a4  src/presentation/analysis/editor_intent.rs
5c84a1b2db472f2205666d529e4768ea71c86802eca3775b88851a9d5cd68634  tests/desktop/library_service/fixture.rs
dc220a629ad3dce5addf6b491ddbc460a183a402e56cab80e0b0ca708700d677  tests/desktop/library_service/open_application_recovery_review_fixture.rs
```
