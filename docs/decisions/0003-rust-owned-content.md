# ADR 0003: Rust-owned content and mechanisms

- Status: Accepted
- Date: 2026-08-14
- Supersedes: the embedded scripting and non-Rust content clauses of
  [ADR 0001](0001-technology-stack.md) and [ADR 0002](0002-prototype-progression.md)

## Context

ADR 0001 introduced an embedded Lua runtime as a boundary for authored rooms and possible future
mechanism callbacks. In practice, that boundary loaded one declarative development room while all
authoritative simulation and generator v6 content already lived in Rust. Runtime callbacks were
never implemented.

The v6 direction is procedural and certificate-driven. A stable Rust key regenerates each room;
offline curation selects keys and records replay evidence in checked manifests. The manually
designed First Steps room is a mechanics fixture, not the beginning of a separate external content
catalogue. Modding, non-programmer content production, and hot reload are not current product
requirements.

Executable mechanism callbacks would also enlarge the deterministic simulation boundary. Any
callback state that affects a future tick would have to clone correctly during AI search,
participate in reset and state hashing, reproduce in replays, and operate without uncontrolled
time, randomness, I/O, or iteration order. Maintaining that machinery without a concrete use is
not justified.

## Decision

Rust owns procedural generation, manually authored fixtures, room mechanisms, and authoritative
behaviour. First Steps is constructed as a typed Rust fixture, and generated rooms construct the
same validated core `Room` model. The application has no runtime scripting engine or general
external room-definition loader.

Complex traps and event-driven mechanisms should be represented first as typed Rust definitions
with explicit, cloneable state. Every definition and state field that can affect simulation must
have documented reset semantics and participate in the appropriate content or state digest.

The v6 catalogue manifests remain external, versioned data. They are offline curation artifacts
containing exact regeneration keys, policy identities, fingerprints, and replay evidence. They do
not form a general gameplay-content boundary and cannot execute behaviour.

If a concrete need later arises for modding, non-programmer authoring, hot reload, or external
tool interoperability, make a new decision based on that need. A declarative format should parse
into versioned transfer types and validate through the existing Rust constructors; an executable
scripting runtime should be added only if data and typed mechanisms cannot express the required
behaviour.

## Alternatives considered

### Keep the embedded scripting runtime

This would preserve an expressive future extension point and could eventually support mods or
rapid authored behaviours. Today it maintains a virtual machine, sandbox, conversion schema, and
native build dependency for a single static room. Future callbacks would add cloning, hashing,
replay, solver-performance, and failure-mode obligations before there is a product requirement
for them.

### Replace room scripts with RON or JSON

A declarative format would remove code execution and could be appropriate for a sizeable authored
catalogue or external tooling. It would still introduce a second schema, migrations, diagnostics,
and conversion code for one development fixture. RON is convenient for internal hand-authored
data; JSON has broader tooling support. Neither is warranted until content must be edited outside
Rust.

### Rust-only content

This aligns authored fixtures with the procedural generator, keeps mechanism state visible to the
compiler and deterministic simulation, and removes an unused runtime boundary. The cost is that
changing First Steps requires recompilation and contributors must be comfortable editing Rust.
Those tradeoffs are acceptable for the current team and prototype.

## Consequences

- Generated rooms and authored fixtures share constructors, validation, content hashing, replay,
  and solver behaviour.
- Mechanism definitions and mutable state remain explicit, cloneable, testable Rust values.
- The build and release no longer carry an unused scripting runtime or its sandboxing surface.
- First Steps changes require a rebuild; there is no hot reload or user-authored mod format.
- Checked catalogue manifests remain reproducible data products rather than executable content.
- Adding a data or scripting boundary later requires a concrete use case and a new compatibility
  and determinism contract; this decision does not prohibit one.
