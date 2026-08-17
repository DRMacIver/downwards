# Design Notes

This directory is the consolidated design record for **downwards** — a Rust
Celeste-like precision platformer crossed with a roguelike dungeon delver. It
was produced on 2026-08-17 by mining every AI development session transcript
(Claude and Codex) for the project, synthesizing the material by category, and
editing it into standalone documents. It exists so that a future developer —
human or AI — can pick up the project cold and understand not just what was
built, but why, what was tried and rejected, and what remains open.

Verbatim quotes attributed to "the designer" are the project owner's own words
from the transcripts, kept where they carry intent better than a paraphrase.
Where sources conflicted, later statements were preferred and real conflicts
are called out in place.

## Contents

| Document | What it covers |
| --- | --- |
| [vision-and-inspirations.md](vision-and-inspirations.md) | What the game is, the descent theme, references (Celeste, Moneyseize, Super Meatboy, Unexplored), feel goals |
| [decision-log.md](decision-log.md) | Dated decision records, including reversals, with context/decision/rationale |
| [playtest-feedback.md](playtest-feedback.md) | The designer's reactions to interim builds and what changed as a result |
| [engineering-notes.md](engineering-notes.md) | Pipelines, tools, regeneration procedures, testing philosophy, agent-workflow lessons, gotchas |
| [research-log.md](research-log.md) | Experiments, metrics, and findings on level generation and difficulty |
| [open-questions-and-todo.md](open-questions-and-todo.md) | Deferred work and unresolved questions |

## Relationship to other documentation

These notes synthesize; they do not replace the primary documents elsewhere in
`docs/`:

- **`docs/decisions/`** — formal ADRs (0001 technology stack, 0002 prototype
  progression, 0003 Rust-owned content / Lua removal).
- **`docs/design/`** — working design artifacts: `room-iteration-playbook.md`,
  `dungeon-v2-rationale.md`, the 2026-08-16 audit/critique/redesign JSON and
  markdown files, `vertical-slice.md`.
- **`docs/research/`** — the procedural generation program, `corpus-plan.md`,
  the human calibration gallery plan, and dated experiment writeups.
- **`docs/validation/`** — dated validation reports for generators, movement
  policies, and the demo dungeon vertical slice.

Where a topic has a primary document, these notes give the narrative and link
to it rather than restating its contents.
