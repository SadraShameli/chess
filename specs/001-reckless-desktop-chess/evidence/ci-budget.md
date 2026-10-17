# CI budget verification (T257)

Status: not verified. This file records what must be confirmed before any automation is enabled.

## Required facts, none confirmed from this workspace

- The actual repository account and plan, and its included Actions minutes and storage allowances.
- Whether standard ARM macOS runners are available to that plan at no extra charge.
- That a hard spending limit of zero is configured so overage cannot bill.
- That release assets for every target fit the release-asset size limit.

## Decision

Because none of these can be read from the local checkout, the workflow templates in `.github/workflow-templates/` stay inert (they are not under `.github/workflows/`), and the local-build path in the repository remains the release method. Enabling automation requires the owner to verify the four facts above and record the numbers here.
