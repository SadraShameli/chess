# Native Library query and Open producer

## Scope and verification state

This checkpoint supplies the T107 query and saved-game Open producer. It does
not establish completion of T107. The parent owns module registration, the
native app mount, the annotation leave guard, application routing, and service
extensions. Favorite assignment and organization mutations still require service
actions and corresponding native controls. This panel offers their filters only.

All ten new tests are UNRUN. No Cargo, tests, build, Clippy, native runtime,
CPU diagnostics, agents, extra worktrees, commits, or pushes were executed for
this checkpoint. Test sources were authored before production source. No failing
baseline execution or passing behavior result is claimed. Pinned standalone
rustfmt and static Git/source inspection are the available verification here.

The canonical rule catalog, all seven rules, global guidance, User Story 3,
FR-004, and the Library workspace contract were read. Engineering policy remains
in [the canonical rules](../../../.specify/rules/README.md).

## Frozen inputs

The checkout began at `0d0bcb0d86fdf37f34fcd6f7fb35dfe3d15a3d68`.
The following absolute patches were hash-verified and applied in order:

| Input | SHA-256 |
| --- | --- |
| `/Users/sadrashameli/Personal/chess/.git/parallel-patches/native-annotation-two-red-baseline-20261017/source.patch` | `4d4685950f2739ca0d3a428e2e9cc424e0ba46dbe2e934c3a03f65c4cbd6ccd9` |
| `/Users/sadrashameli/Personal/chess/.git/parallel-patches/library-application-routing-20261017/source.patch` | `b29d70fa5f86b7dbf190b054732ab1ef05dfd33d99a0442c5c0e796483194dea` |

Neither dependency belongs in the incremental export. The export contains only
new Library presentation source, its included test sources, and this document.

The parent supplied the canonical service error contract during implementation:
`DesktopLibraryServiceEnumDisposition::{Refused, Retained { request, expected }}`,
`DesktopLibraryServiceError::Continuation { request, expected, source }`, and
`DesktopLibraryServiceError::disposition()`. Owned observation source consumes
that contract. It is not present in either frozen input above. The corresponding
service repair must be integrated by the parent before compilation. No local
service mirror, fabricated disposition implementation, or shared protocol edit
is included in this checkpoint.

## Integration surface

`PresentationLibraryPanel::new(workspace)` owns the controller and filter draft.
`show(ui)` returns `Vec<PresentationLibraryEnumEffect>`. `Dispatch(intent)`
contains Query or Metadata work. `OpenSaved(intent)` contains an immutable Open
operation with the exact selected `GameId` and actual `GameStorageRevision`.

`controller()` and `controller_mut()` expose the integration contract:

1. Hold the emitted Open intent through the parent's existing annotation guard.
   Save/Discard/Stay decisions must never replace its game or content revision
   with a later selection. Stay may call `abandon_intent(&intent)` before binding
   an application request. The component never switches annotation sources.
2. Create the application request with
   `port.request(DesktopCommand::library(intent.operation().clone()))`.
   The intent's `input_id()` identifies UI input. The application allocates a
   different request identity. No equality between those identities is required.
3. Call `attach_request(&intent, request.clone())` before submission. Retrying
   application admission must retain that exact request, not allocate another.
4. After actual admission, call `attach_receipt(&intent, receipt.clone())`.
   A replacement request or a receipt from another outcome is rejected.
5. Forward successful service values through
   `observe_result(&receipt, &original_expected_operation, &result)` and typed
   service failures through
   `observe_error(&receipt, &original_expected_operation, error)`.
   Query and metadata values are actual canonical repository/service pages.
   The parent continues handling Opened publications and editor presentation.

The `retained_request()` and `retained_receipt()` getters preserve identity and
ownership evidence. They do not provide a continuation mechanism. Resubmitting a
completed application request returns its cached terminal response. The panel
therefore offers no retry control that claims to resume retained Open work.
Recovery requires the canonical service/root continuation seam.

Refused failures retire only the operation with the bound actual receipt.
Retained failures preserve the bound original request, receipt, and operation.
The current producer accepts a retained certification for its actual request
with no editor context; it rejects foreign request/context certifications.
No generic `DesktopFailure` is interpreted as a service disposition.

## Query and navigation behavior

The query draft builds `StorageLibraryQueryOptions` and validates it through the
canonical `StorageLibraryQuery` boundary. Controls cover either player, White,
Black, complete date interval, result, rules variant, starting-position kind,
opponent mode, favorites, any/all selected tags, folder/Unfiled/subtree, opening
name/code, half-move interval, and all six canonical sorts. The initial sort is
`UpdatedDescending`. Search is explicitly labelled as player search; annotation
text search is not offered.

Only one actual query page is retained. Next uses its actual opaque cursor and
the original canonical options; it does not synthesize offsets or page rows.
Loading replaces prior rows. Stale/invalid cursor errors remove stale results,
show the typed cause, and offer explicit first-page reload. Reads never issue a
save or organization mutation, so this component does not change modified time.

Folder navigation uses canonical Folders requests with parent and cursor,
canonical Folder reads with catalog revision for parent navigation, and a root
navigation control. Tag navigation uses canonical Tags cursors. Each metadata
category retains a single bounded page, plus one folder-detail read; it never
loads the complete hierarchy or accumulates pages. Selected tags retain bounded
canonical IDs, including selections from previous metadata pages. A name absent
from loaded matching metadata is explicitly unavailable and shows its actual
identity. Missing metadata is distinct from a successfully loaded empty page.

Rows display actual players, partial dates, result, variant, start kind,
available opponent mode, opening, half-move length, modified timestamp,
favorite/filter metadata, and saved-game identity. Missing opponent mode is
unavailable, including notation sources, rather than an invented played mode.
Unindexed documents are explicitly reported; a filtered empty page does not
claim coverage of unindexed documents. No Start or Resume command is emitted.

The controller permits one pending action in each of five typed lanes: Query,
Folders, Tags, Folder detail, and Open. The input ledger retains pending inputs
and a rolling 256 completed inputs. Completed IDs are evicted instead of imposing
a lifetime limit. Egui input effects are suppressed during sizing passes and
deduplicated by frame and widget identity across repeated passes.

## Authored test contracts, all UNRUN

| Test | Intended evidence |
| --- | --- |
| `real_pages_keep_default_sort_and_exact_continuations` | Temporary SQLite and actual service Query pages return 100 then 1 distinct rows; recent modification controls initial order and Next carries the actual cursor. |
| `stale_cursor_requires_explicit_first_page_resync` | A real catalog change rejects a continuation, hides stale rows, and refresh starts without a cursor. |
| `exact_request_and_receipt_survive_guard_and_reject_replacement` | Separate input/application identities, request replacement refusal, frozen Open through query change, and real played-session paused restoration. |
| `canonical_metadata_pages_remain_bounded_and_navigate_nested_folders` | Actual 100/1 tag pages, real folder names, nested folder reads, and bounded child navigation. |
| `recent_input_ledger_evicts_completed_inputs_without_lifetime_lockout` | More than 256 completed inputs remain usable while recent duplicate inputs are suppressed. |
| `foreign_receipt_cannot_publish_a_real_repository_page` | A real admitted foreign receipt cannot publish an actual repository page to another action. |
| `retained_open_failure_keeps_exact_original_and_rejects_foreign_continuation` | Typed continuation correlation preserves actual admitted ownership and blocks replacement or abandonment. |
| `query_producer_forwards_real_filters_without_fabricating_opponent_metadata` | Canonical options reach real SQLite/service filtering and notation has unavailable opponent mode. |
| `repeated_egui_passes_emit_initial_reads_once_and_show_real_empty_state` | Repeated native UI passes emit initial reads once and actual empty pages produce the empty state. |
| `native_player_filter_and_open_emit_once_with_truthful_notation_labels` | Real notation rows keep honest mode labels; native text/filter/Open input emits once and Open remains unadmitted for the parent's guard. |

Tests are included only through the owned source module and the new
`tests/presentation/library.rs` aggregation file. They reuse the frozen canonical
service fixture, real temporary SQLite documents, actual application admission,
and repository/service pages. They introduce no public page constructors and
perform no implicit network operation. Fixture cleanup precedes final observation
assertions; setup/contract assertions inside observation blocks remain unrun.

## Remaining acceptance

The parent must register and mount the module, wire every emitted intent and
typed service completion, finish the canonical retained Open continuation seam,
and keep the root annotation Save/Discard/Stay guard authoritative. Favorite
assignment, tag assignment, folder mutations, and their service actions remain
unimplemented by this slice. Native root composition, behavioral tests, Clippy,
builds, runtime accessibility/platform checks, and release verification remain
unverified. Task and checklist markers were not changed.
