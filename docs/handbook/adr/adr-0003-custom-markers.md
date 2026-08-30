# ADR-0003: Regen-safety via CUSTOM markers and `user_owned`

- **Status:** Accepted
- **Date:** 2026-07-03
- **Deciders:** Backbone Framework maintainers

## Context

[ADR-0001](adr-0001-schema-yaml-ssot.md) makes the schema authoritative and regeneration
repeatable — `generate --force` overwrites downstream code. But real modules always accumulate
hand-written logic: a business rule, an extra DTO, a non-CRUD endpoint. If regeneration destroyed
that logic, developers would stop regenerating (to protect their code), and the source-of-truth
guarantee would quietly collapse. Generated and hand-written code must **coexist in the same tree**,
across arbitrarily many regenerations, without either clobbering the other.

## Decision

**Three layered mechanisms mark code the generator must preserve.** Pick by scope.

1. **`// <<< CUSTOM … // END CUSTOM` markers** — inline regions inside a generated file. Content
   between the markers survives regeneration. For small additions (a method, a re-export, an extra
   DTO). Marker spelling varies by file and must be matched, not invented.
2. **`*_custom.rs` sibling files** — filenames the generator never emits, so it never overwrites
   them. For cohesive units of logic (a custom service). Registered from the surrounding `mod.rs`
   *inside* a CUSTOM marker so the `mod` line survives too.
3. **`user_owned` globs in `metaphor.codegen.yaml`** — paths the generator skips wholesale (never
   reads, merges, or deletes). For whole hand-owned subtrees. The skeleton ships
   `tests/features/**` and `docs/**` protected this way.

## Alternatives considered

- **Separate hand-written crate / module** that imports the generated one. Clean isolation, but
  forces an awkward two-crate split and cannot express "one extra method on this generated struct."
  Rejected as too coarse.
- **Three-way merge on regeneration** (like a scaffolder that diffs). Powerful but fragile —
  conflict resolution on generated code is exactly the pain we are avoiding, and it is
  non-deterministic. Rejected.
- **Never regenerate after first generation** (one-shot scaffolding). Abandons the source-of-truth
  guarantee the moment logic is added. Rejected — this is the model Backbone explicitly improves on.
- **A single mechanism** (markers only, or `user_owned` only). Markers alone cannot protect a whole
  new file's name; `user_owned` alone cannot protect a few lines inside a regenerated file. Both
  scopes are needed. Rejected in favor of the layered set.

## Consequences

**Easier:** developers regenerate freely and forever without fear; custom logic and generated code
live side by side; the scope ladder (marker → file → glob) maps cleanly to the size of the change.

**Harder / to live with:** developers must *know* which mechanism applies and use it — code left in
unprotected generated territory is silently overwritten (the single most common regression, called
out in the [Maintainer Guide](../05-maintainer-guide.md) and the PR checklist); marker spelling
differs across generated files and must be matched exactly; `user_owned` entries are a manual list
that can fall out of date if a protected path moves.
