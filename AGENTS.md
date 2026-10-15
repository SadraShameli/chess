# Project instructions

Read `~/.claude/CLAUDE.md` before project work. Read applicable nested instruction
files and any applicable user-level path-scoped rules.

Read `.specify/memory/constitution.md`, then `.specify/rules/README.md` and the
canonical rule files it requires for the task. Follow the same rule-loading
procedure for ordinary development as for Spec Kit work. Rules live only in
`.specify/rules/`; do not duplicate them here, in the constitution, in feature
artifacts, or in agent-specific instruction folders.

Spec Kit skills for Codex live in `.agents/skills/speckit-*/SKILL.md`. When one is
requested, read and execute that skill's procedure using the available tools.
Treat its argument placeholder as the user's supplied instructions. Codex and
Claude Code share the same constitution and rules; Claude skills are installed
under `.claude/skills/`. Follow the work-authorization policy in the verification
rule.
