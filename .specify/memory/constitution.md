# Chess Constitution

## Core Principles

### I. Canonical Engineering Rules by Reference

The normative Rust engineering policy is
[the canonical rule catalog](../rules/README.md) and the rule files it requires
for the current task. Those files are incorporated into this constitution with
the same authority as inline principles.

Before specification, planning, source work, configuration changes, testing, or
review, the agent MUST open the catalog and read every applicable rule in full.
Reading only this constitution, relying on a remembered summary, or following a
generated template without loading those files does not satisfy this requirement.

Engineering obligations MUST be maintained in their owning rule file. This
constitution, agent guidance, and feature artifacts MUST link to those rules
rather than duplicate their text. Feature artifacts record concrete decisions,
compliance evidence, and measured outcomes.

## Project Configuration

The configuration inventory and verification entry points are owned by
[the verification rule](../rules/verification.md). Agents MUST inspect the actual
configuration files when changing or assessing them. Runtime behavior,
optimization claims, and supported targets MUST be established by implementation
and evidence, not inferred from configuration or policy alone.

## Development Workflow and Review

Use the installed Spec Kit workflow for feature work, keeping feature artifacts
under `specs/`. The user-facing invocation guide lives in
[the project README](../../README.md). Constitution updates are governance work;
implementation or refactoring requires its own authorized task.

Before implementation, each plan's Constitution Check MUST name the applicable
rule files and show how its design meets them. Recheck after design. Tasks and
reviews MUST verify the same obligations and identify any unresolved discrepancy.
A template's complexity section does not authorize an exception to a rule.

The catalog is also the rule-loading entry point for ordinary development through
[AGENTS.md](../../AGENTS.md) and [the Claude bridge](../../.claude/CLAUDE.md).
Spec Kit and direct development use the same policy.

## Governance

Explicit owner instructions take precedence. Read the global owner guidance at
`~/.claude/CLAUDE.md` and any applicable nested instruction files. Codex and Claude
Code share this constitution and its referenced rules. The project has no
dependency on the TradingBot checkout.

Amendments MUST identify the changed rule owners, rationale, compatibility impact,
and verification or migration needs. Update the canonical rule rather than
creating a second copy. Review affected configuration and active feature
artifacts for consistency and report unresolved dependent work.

The constitution version covers changes to incorporated rules as well as this
document. Use MAJOR for incompatible removal or redefinition of an obligation,
MINOR for new or materially expanded requirements, and PATCH for clarifications
that preserve obligations. Update Last Amended whenever either changes and keep
the original Ratified date.

Each amendment MUST include a temporary sync impact report for review. Keep it in
visible Markdown under the code-quality policy and remove it before committing.
The initial adoption was 1.0.0. Amendment history is tracked through versioned
changes to this document and its incorporated rules.

**Version**: 1.1.3 | **Ratified**: 2026-10-15 | **Last Amended**: 2026-10-17
