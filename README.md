# Chess

A Rust edition 2024 project. The application currently contains the Cargo starter
binary; feature design begins with a specification.

The [Spec Kit constitution](.specify/memory/constitution.md) incorporates the
[canonical Rust rules](.specify/rules/README.md) by reference. [AGENTS.md](AGENTS.md)
and the [Claude bridge](.claude/CLAUDE.md) direct both agents to read them for
ordinary development as well as Spec Kit workflows. Engineering requirements
are maintained in those rule files only.

The pinned toolchain, Cargo lint and optimization settings, formatter, Clippy
thresholds, and [verification workflows](.specify/rules/verification.md) are
configured. Run `make verify` for the complete verification sequence or
`make profile-build` for an optimized build with full debug information.

Run the application with `make run`. Pass application arguments with
`make run ARGS="your arguments"`.

## Spec Kit

[GitHub Spec Kit](https://github.com/github/spec-kit) v1.2.0 is initialized here
for Codex and Claude Code. The shared `specify` CLI is installed through uv from
the stable `v1.2.0` tag. Codex remains the default integration.

The constitution is established. Start a feature by invoking these skills in
your agent's chat, with a description after `speckit-specify`:

| Step | Codex skill | Claude Code skill |
| --- | --- | --- |
| Specify the feature | `$speckit-specify` | `/speckit-specify` |
| Clarify open questions, if needed | `$speckit-clarify` | `/speckit-clarify` |
| Plan the implementation | `$speckit-plan` | `/speckit-plan` |
| Generate tasks | `$speckit-tasks` | `/speckit-tasks` |
| Implement | `$speckit-implement` | `/speckit-implement` |
| Assess remaining work | `$speckit-converge` | `/speckit-converge` |

Repeat implementation and convergence until the feature converges. Skills are
chat instructions, not terminal commands. Newly installed skills are available
on the next agent turn. Use `speckit-constitution` to amend project principles.

Feature artifacts belong in `specs/`. The toolkit's scripts and templates live
in `.specify/`; installed skills live in `.agents/skills/` for Codex and
`.claude/skills/` for Claude Code.

Run `specify integration status` to inspect the local setup.
