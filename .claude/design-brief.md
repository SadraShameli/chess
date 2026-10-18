# Reckless Chess: UI redesign brief

Design a premium, calm, offline desktop chess app for macOS, Windows and Linux.
Think "a well-made pro tool for serious players": the board is the hero, chrome
gets out of the way, everything is keyboard-reachable.

## Product

- Offline study and play against a bundled strong engine (Reckless), plus local
  two-player and up to several concurrent games in tabs.
- A real-time 3D board (rendered pieces, ivory and ebony satin finish, gentle
  camera tilt and rotate) in the center of the window. Everything else is 2D UI
  around it.
- Desktop window, typical sizes 1280x800, 1440x900 and large monitors. UI scales
  at 100, 150 and 200 percent.

## Workspaces (top-level navigation)

1. **Games**: new game setup (time control, opponent, Chess960, engine strength),
   game tabs with a live "all games" overview (turn, clocks, paused/unsaved
   state), the live board, two clocks, move list, Pause/Resume, Resign, Draw,
   Takeback, premoves, promotion picker, Return to Live while browsing history.
2. **Puzzles**: run setup (rating band, themes, countdown), the puzzle board,
   session timer, Hint, Skip, Next, Retry, End Run confirmation, run summary,
   saved-run history with delete.
3. **Library**: search and filters, folders, tags, favorites, Trash, PGN
   import with duplicate preview, export, opening a saved game for study.
4. **Editor / Study**: position editor, variation tree, annotations and notes,
   engine analysis panel (eval bar, lines, depth, Start/Stop), game report
   (accuracy, mistakes), opening explorer (continuations with frequency bars,
   sample games, Next Samples) and opening book.

Plus: **Settings** (appearance light/dark, motion, engine threads and hash,
shortcuts, audio, datasets, backups and restore, updates), a **command palette**
(Cmd/Ctrl+K), modal confirmations, and a graphics-recovery banner.

## Layout ideas to explore

- Slim left rail for the four workspaces and Settings, icons with labels.
- Board centered and as large as possible; a right inspector panel (about 360
  px, resizable, collapsible) holding the workspace's tools.
- Clocks and player names attached to the board edges, not floating in panels.
- Game tabs along the top when several games run.

## Visual direction

- Dark theme first, with an equally finished light theme.
- Warm neutral surfaces, one restrained accent, high-contrast type; no
  gradients-everywhere, no glassmorphism soup.
- Typography: Inter (bundled). Monospace or tabular figures for clocks, evals
  and move numbers. Figurine notation in the move list.
- Status colors only for meaning: running clock, low time, engine error,
  unsaved.
- WCAG AA contrast, minimum 28 px hit targets, visible focus rings, reduced
  motion option.

## Deliverables

1. A small design system: color tokens (dark and light), type scale, spacing,
   radii, elevation, buttons, inputs, tabs, segmented controls, list rows,
   modals, toasts, status pills, eval bar, clock.
2. Hi-fi screens: Games (one live game), Games (overview with 3 games),
   Puzzles (active run), Library, Study with engine analysis and explorer,
   Settings, command palette, New Game dialog, End Run confirmation.
