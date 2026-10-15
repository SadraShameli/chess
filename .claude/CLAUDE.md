# Project instruction bridge

Read `~/.claude/CLAUDE.md`, then the
[project constitution](../.specify/memory/constitution.md) and
[canonical rule catalog](../.specify/rules/README.md). Load the complete rule files
required by the catalog for the current task, including ordinary development.

Engineering rules live only in `.specify/rules/`. This file is a discovery bridge;
do not copy those rules into Claude-specific files. Spec Kit skills for Claude
Code are installed under `.claude/skills/`.
