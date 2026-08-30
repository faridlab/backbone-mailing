# mailing — Handbook

The documentation set for the **`mailing`** Backbone Framework domain module.

> **This handbook is stamped from the [module skeleton][skel].** In the unmodified skeleton,
> `mailing` is `backbone-module-skeleton` (v0.1.3) and the reference entity is `Example` — one
> entity wired end-to-end that you rename to your own domain concept. Because `docs/**` is a
> `user_owned` path, this handbook is copied into your module and is yours to adapt: start with
> **[Adapting this handbook to your module](handbook/00-adapting-to-your-module.md)**.

[skel]: ../README.md

Every page below names **one reader** and **one mode** (Diátaxis) at its top. Find your reader,
follow the path.

## Find your path

| You are… | You want to… | Start here |
|----------|--------------|-----------|
| **Evaluator** | Decide whether to build on this | [Philosophy](handbook/01-philosophy.md) → [Background](handbook/02-background.md) → [Technology](handbook/03-technology.md) |
| **App developer** | Ship a module and integrate it | [Developer Guide](handbook/06-developer-guide.md) |
| **Maintainer** | Understand the machine and extend it safely | [Architecture](handbook/04-architecture.md) → [Maintainer Guide](handbook/05-maintainer-guide.md) |
| **Contributor** | Open a correct PR | [Contributing](handbook/07-contributing.md) |
| **Anyone** | Agree on what a word means | [Glossary](handbook/08-glossary.md) |

## The handbook

0. [Adapting this handbook to your module](handbook/00-adapting-to-your-module.md) — *Maintainer.* Read first when stamping a new module: what `mailing` means, which pages to keep verbatim vs. fill in.
1. [Philosophy & motivation](handbook/01-philosophy.md) — *Evaluator.* What problem a module solves, the worldview, and the non-goals.
2. [Background & prior art](handbook/02-background.md) — *Evaluator.* What came before (hand-rolled CRUD, ORMs, scaffolders) and what this rejects.
3. [Technology & the "why"](handbook/03-technology.md) — *Evaluator + Maintainer.* The stack, each choice with a rationale and a rejected alternative.
4. [Architecture](handbook/04-architecture.md) — *Maintainer.* C4 view: context, containers, the DDD 4-layer shape, and a request traced end-to-end.
5. [Maintainer Guide](handbook/05-maintainer-guide.md) — *Maintainer.* Schema-YAML SSoT, regeneration, `// <<< CUSTOM` markers, where code goes per layer, release flow.
6. [Developer Guide](handbook/06-developer-guide.md) — *App developer.* Install → quickstart → recipes → configuration → troubleshooting.
7. [Contributing](handbook/07-contributing.md) — *Contributor.* Dev setup, commit/PR conventions, tests and lint, review checklist.
8. [Glossary](handbook/08-glossary.md) — *All.* One term, one meaning, used everywhere.
9. [Architecture Decision Records](handbook/adr/) — *Maintainer.* Why this design, not another.

## Related, already-written docs

This handbook is the *narrative*. Two reference sets live alongside it — link out, don't duplicate:

- **[Schema DSL reference](schema/README.md)** — the exact YAML grammar: [types](schema/TYPES.md), [model rules](schema/RULE_FORMAT_MODELS.md), [generation targets](schema/GENERATION.md), [error codes](schema/ERROR_CODES.md), [examples](schema/EXAMPLES.md). This is the *Reference* corner of Diátaxis; the handbook explains the *why*.
- **[Business flows](business-flows/README.md)** — one doc per business flow (actors, preconditions, rules, postconditions), each linked to its executable BDD oracle.

## Conventions this handbook follows

- **Reader + mode named** at the top of every page.
- **Commands are real.** Every `metaphor …` command was run against `metaphor 0.2.0` while writing. Where a command in the top-level [README](../README.md) is stale, the handbook flags it and gives the working form.
- **Code wins over docs.** When a doc and the schema/code disagree, the schema YAML (the source of truth) wins — the doc is the bug.
