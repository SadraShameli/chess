# Native workbench export owner

This T083/T086/T089 implementation slice adds the native export owner over the
existing [canonical diagnostic encoder](workbench-export.md). It follows the
[constitution](../../../.specify/memory/constitution.md), its complete
[rule catalog](../../../.specify/rules/README.md), and the
[desktop lifecycle contract](../contracts/desktop-ipc.md). Parent-owned actor,
command, coordinator, bridge, composition, module and task wiring is excluded.

## Immutable preview and identity

`DesktopWorkbenchExport::preview(request, result)` captures the actual terminal
`DesktopWorkbenchCommandsResult::job()` and its `export_context()`. The native
owner never rebuilds content from presentation rows. The request UUID identifies
the export and preview; the compiled package version supplies application
identity. `WorkbenchExportMetadata::with_optional_timestamp` validates these
fields while representing an unavailable wall timestamp as `None`. No wall clock
or invented zero is read. The existing required-timestamp constructor delegates
to the same validation.

One retained `Arc<DesktopWorkbenchExportPreview>` owns the exact encoded bytes,
length, SHA-256, job identity and retained/evicted diagnostic counts. Repeating
the same preview action and job returns that same allocation. A newer preview
invalidates the old preview ID for publication; externally retained old previews
remain immutable. A preview cannot replace a pending native write.

## Publication and cancellation

`DesktopWorkbenchExportRequest` carries the preview ID, expected SHA-256,
coordinator-consumed `DesktopFileEnumSelection` and application receipt.
`begin_apply(request, executor)` validates preview identity and digest before
submitting one operation to the injected shared `CapacityExecutor`. Capacity
refusal starts no file work. Destination conversion, staging, writes, flush,
file synchronization, target validation, publication and directory
synchronization all run on that executor. The native adapter implements the
lower-owned `WorkbenchExportPort` and writes the frozen artifact unchanged.

`StorageAtomicFileStage::commit_if(cancelled)` checks the closure after staged
flush/fsync and current target validation, immediately before `persist` or
`persist_noclobber`. `commit()` retains its unconditional behavior through that
same implementation. The native guard observes both receipt cancellation and
executor cancellation. Cancellation at this boundary returns
`StorageAtomicFileError::Cancelled`, drops the staging owner and returns no
export receipt. Validation and I/O failures retain their typed sources and are
not replaced by a concurrent cancellation flag.

Postpublication synchronization errors become
`StorageAtomicFileError::PublicationIndeterminate`; publication is not rolled
back or reported as canceled. The export owner preserves that error and refuses
another write, a new preview and successful shutdown until the outcome is
reconciled. Loss of the executor's completion result is likewise an explicit
unresolved write, rather than permission to replay a possible publication.
This slice does not implement filesystem reconciliation or global
receipt-cancellation serialization. A flag arriving after the guard cannot
promise cancellation of publication; the eventual real commit result remains
authoritative.

## Retained work and retries

`observe().await` borrows a pinned future retained by the owner. That future owns
the `CapacityExecutorJob` and its native selection/stage lifecycle. Dropping an
observation or an `apply().await` wait does not drop the retained future, detach
the job or permit a second write. A new request while writing is refused.

Completion retains the application receipt and exact committed export receipt,
or the typed source error available through `error()`. Repeating observation
returns the same outcome. Reusing the original receipt and matching preview and
digest replays the stored outcome without touching another supplied selection.
An ordinary failed write may be attempted again with a new action receipt and
fresh destination selection. Capacity refusal may be retried after capacity is
available. An indeterminate publication cannot be blindly replayed.

`shutdown_retained(deadline)` requests receipt cancellation and observes the
existing future against the absolute injected deadline. Deadline expiry leaves
the future, receipt and selection lifecycle owned and pending. A later
observation reconciles its real outcome. A successful publication observed
after a cancellation request keeps its exact receipt. The parent must keep the
owner alive through pending shutdown and deliver an already committed receipt
without relabeling it as an uncommitted cancellation.

## Deterministic coverage and verification limits

Nine tests in `tests/desktop/workbench_export.rs` cover actual terminal child
provenance, exact frozen bytes/digest/metadata, stale and unknown preview IDs,
wrong hashes, destination type refusal, stale native selections, capacity
refusal and retry, exact receipt replay without unrelated target changes,
partial writes and commit failures with source preservation, dropped waits,
pending capacity ownership, shutdown deadline expiry, cancellation at the real
publication guard, and retained indeterminate outcomes. The source includes
this file as a private `cfg(test)` module, permitting controlled native-port
gates and owned-future assertions without widening the production API. Do not
also register it as an integration module. The indeterminate lifecycle test
injects the typed postpublication error; it does not claim an actual OS fsync
failure was induced.

Three additional tests in the existing canonical
`tests/storage/atomic_file.rs` cover both existing/new targets canceled at the
guard, validation preceding cancellation, and a deterministic native publication
failure preserving its source. Its Cargo-discovered root is `tests/storage.rs`.
All new filesystem paths are isolated temporary paths. Gates use channels and
future polling, with no sleeps, network or user data.

Executed locally: standalone Homebrew Rust 1.99.0 toolchain identification,
`rustfmt` 1.10.0 on only the five owned Rust files, formatter check mode, and
scoped Git whitespace/patch checks. Cargo, Clippy, tests and builds were not run
in this lane because the parent owns serialized execution. No red test
execution, passing test count, runtime performance or complete native
integration acceptance is claimed. The prerequisite script succeeds when given
the explicit existing feature directory; its built-in requirements checklist
has 16 checked entries and native-desktop checklist has 40 checked entries.
The reviewer artifact contains no checkbox gate.

The dependency snapshot `native-engine-composition-base.patch` was applied only
as local setup. The deliverable patch contains only this evidence file,
`src/desktop/workbench_export.rs`, the narrow changes to
`src/workbench/export.rs` and `src/storage/atomic_file.rs`, and the two owned test
paths. Add `pub mod workbench_export` at the desktop module root in the parent.
Parent command/bridge/coordinator wiring uses `PreviewExport { job }`,
`ApplyExport { preview, digest, destination }`,
`ExportPreview(Arc<DesktopWorkbenchExportPreview>)`, and
`Exported { preview, receipt }` according to the agreed handoff.
