# Library service and running-registry saved opening

This is a source-only handoff for the first admitted US3 service surface under
T096/T106/T110. No task marker is changed. The authored Rust has been formatted
and inspected statically. No Cargo, test, build, Clippy, native execution,
performance measurement, or runtime diagnostic was run in this lane.

The export starts from parent commit
`465837a95c0de9129460171ea57a5edb2d054aac` plus dependency tree
`3af7406383b36908adae237eba518ef801d92c13`. The dependency patch SHA-256 is
`3a8fd30ab4d987148dad1678c08afaf4b40cc0e368ded496cba408b448a96d67`.
The incremental patch excludes dependency edits and parent-owned registration,
messages, DTOs, identity, coordinator, bootstrap, lifecycle mounting, supervisor,
UI, scene and input files.

## Composition and request contract

`DesktopLibraryService::new(DesktopLibraryServiceOptions)` returns the actor and
`DesktopLibraryServicePort`. Options inject one bootstrap-minted workspace
`DesktopMessagesEntityId`, `StorageDocumentClient`, `Arc<CapacityExecutor>`,
`DesktopGameRegistryPort`, and `Arc<dyn GameStorageWallTimeProvider>`.

`DesktopLibraryServiceOperation::new(workspace, action)` wraps the Clone/Debug/Eq
`DesktopLibraryServiceEnumAction`: Query with options/cursor, Metadata with the
canonical request, Open with game/storage revision, and Editor with complete
context/action. The request constructor takes the actual application receipt
first and operation second. The provider's `execute` returns a boxed Send static
future yielding the result or `Arc<DesktopLibraryServiceError>`.

The operation owns `mutation_entity`, `is_available_while_closing`, and
`expected_revision`. The last method returns
`Result<Option<DesktopRevision>, DesktopError>`, mapping Open from the supplied
storage revision and Editor from the source's actual game revision. Result
`revision` has the same return type and maps query/metadata catalog revisions or
actual editor source revisions. Reads have no mutation entity; editing,
navigation, preview, checkpoint, retry and opening identify the actual game.

Results are Query with an Arc page, canonical Metadata, Opened publication,
canonical Editor result, PendingSnapshot with a publication/pending state, or
Saved with a publication and an actual Played `GameStorageReceipt` or Notation
`Arc<StorageLibraryContentReceipt>` plus covered `GameRevision`. Debug output
summarizes identity, row counts and state without expanding content graphs.

The service retains bounded admitted work, original receipts, notation owners,
publication/save continuations, and played receipts. It polls owned futures on
the existing runtime and routes request cloning, query normalization, repository
recovery and canonical editor work through the injected executor. It creates
no runtime, engine child, played session, or detached task. Notation Changed
results continue through Checkpoint using the original receipt and the
source-produced updated context. Failed publication recovery resumes publication;
it does not resubmit the edit. Actual storage acknowledgments are returned after
canonical save projection has been observed.

## Running registry boundary

`try_open_saved_editor(game, storage_revision, application_receipt)` admits a
bounded database opening. `try_open_recovered_editor(recovered,
expected_storage_revision, application_receipt)` moves already recovered
ownership into the same actor, avoiding another decoded played session. Both
return `DesktopGameRegistrySavedOpenReceipt`; its mutable wait returns the actual
annotation result or Arc registry error. `application_receipt` and
`editor_receipt` expose the original receipt and actual supervisor receipt.

The actor serializes saved openings with creation and controls, validates exact
stored revision and Library location for absent owners, and uses the existing
supervisor factory/options/install path. It forwards editor Open without Start,
Resume, clock mutation or a search request. Existing owners remain authoritative
and retain their epoch and current source. Loading, constructing, installing and
editor-wait phases remain in the registry across abandoned waits and deadline
failures. Source reservations remain registry-owned through observed cleanup.

## Lifecycle

`run(&mut self)` lets the root own a task returning `(actor, result)`.
`port.begin_closing()` rejects fresh unrelated work while preserving read and
save-recovery operations. `port.try_shutdown(absolute_deadline)` uses a reserved
control channel and returns a mutable-wait shutdown receipt. Run returns after
the shutdown attempt, retaining the actor on failure. The root can then call
`shutdown_retained(&mut self, absolute_deadline)` again. Incomplete CPU, storage,
publication and dirty source state prevents `is_shutdown_complete`. Successful
cleanup observes admitted channel work before releasing canonical source slots.

## Parent prerequisites and tests-only finding

The root must add the agreed narrow recovery transfer to
`src/storage/library_repository.rs`:

```rust
pub fn into_retained_record(self) -> (StorageLibraryRecord, StorageLibrarySourceMemory)
```

`StorageLibrarySourceMemory` is a public opaque non-Clone owner of the existing
`StorageLibraryMemory`, with no second full-record allocation. The new notation
slot keeps it beside the annotation owner; the registry retains it beside the
saved supervisor. This patch consumes that API but does not edit the repository.

The existing annotation owner rejects a canceled receipt before considering a
Checkpoint continuation for content already accepted under that same receipt.
The tests-only case
`accepted_annotation_save_continuation_keeps_its_cancelled_original_receipt`
in `tests/desktop/library_service/annotation_checkpoint_regressions.rs` exercises
the real owner and expects the original receipt to continue through observed
durable saving. It has not been run red here. The parent must observe its
semantic failure before repairing the existing owner. This lane makes no owner
or supervisor repair. The parent's separate shutdown/publication work is not
included in this export.

## Authored coverage, all unrun

| Source-mirrored group | New cases | Intended evidence |
| --- | ---: | --- |
| Service contract | 6 | Query/metadata, actual receipts/UTC, both kinds and epochs, stale contexts, abandoned waits, workspace/closing and deadlines |
| Editor | 5 | Full binding checks, canonical tree/children/proof/scope/confirm, navigation, raw notation warnings/clocks, Trash refusal |
| Recovery | 6 | Newest source/UTC retry, publication-only retry, pending snapshots, bounded admission, returned actor cleanup, source reservation lifetime |
| Pagination | 2 | Actual 101-row query and metadata pages and stale catalog refusal |
| Existing-owner regression | 1 | Canceled original receipt after content acceptance, tests-only for parent red |
| Running registry saved opening | 7 | Completed/paused/live state, exact-once installation, stale storage refusal, abandoned waits, bounded queue and retained shutdown/load/editor receipt |
| Total | 27 | Authored, zero compiled or executed in this lane |

Root registration and parent compilation, semantic red/green execution, all
verification gates, mounted native behavior, all 13 journeys, platform and
release acceptance remain parent-owned. Static formatting and patch checks do
not establish any of those results.
