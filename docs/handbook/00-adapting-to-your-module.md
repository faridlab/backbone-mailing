<!-- Reader: Maintainer · Mode: How-to -->
# Adapting this handbook to your module

This handbook ships **inside the module skeleton** and is copied into every new module you scaffold
from it — `docs/**` is a `user_owned` path in [`metaphor.codegen.yaml`](../../metaphor.codegen.yaml),
so the generator never rewrites it. That means the docs travel with your module and are **yours to
adapt**, exactly like the schema and code.

To keep the handbook generic, it uses the same placeholder convention as the rest of the skeleton:

| Placeholder | Means | Also appears in |
|-------------|-------|-----------------|
| `mailing` | Your module name (lowercase, e.g. `payments`) | `index.model.yaml`, `metaphor.codegen.yaml`, `business-flows/README.md`, `hooks`, `workflows` |
| `Example` / `examples` / `ExampleStatus` | The **reference entity** — the one wired end-to-end that you rename to your real domain concept | `schema/models/example.model.yaml`, the generated `src/` tree, the migrations |

`mailing` is the token the skeleton stamps to your real module name; `Example` stays concrete
because the shipped code, migrations, and schema all use it — you rename it the same way you rename
[`example.model.yaml`](../../schema/models/example.model.yaml) (whose own comment says *"Replace
`example` with your real entities"*).

## When you stamp a new module — the checklist

- [ ] Replace `mailing` with your module name everywhere it appears (the skeleton's
      module-creation step does this for `index.model.yaml` / `metaphor.codegen.yaml`; do the same
      across `docs/` — a single find-and-replace).
- [ ] **Rewrite, keep verbatim, or fill** each page for its reader:

| Page | What to do when adapting |
|------|--------------------------|
| [01-philosophy](01-philosophy.md) | **Keep verbatim** — generic to every module. Optionally add a paragraph on *your* domain's north star. |
| [02-background](02-background.md) | **Keep verbatim** — framework-level prior art. |
| [03-technology](03-technology.md) | **Keep verbatim**, then add any deps your module pulls in beyond the skeleton's. |
| [04-architecture](04-architecture.md) | **Update the entity-specific bits**: the sequence diagram and the layer table reference `Example`; swap in your real entity once it exists. The 4-layer shape and C4 context stay. |
| [05-maintainer-guide](05-maintainer-guide.md) | **Keep verbatim** — the regen workflow and rules are module-agnostic. Commands already use `mailing`. |
| [06-developer-guide](06-developer-guide.md) | **Fill in** your entity's real recipes, config, and troubleshooting once `Example` is renamed. |
| [07-contributing](07-contributing.md) | **Keep verbatim** — conventions are workspace-wide. |
| [08-glossary](08-glossary.md) | **Extend** — add your domain's ubiquitous-language terms; keep the framework terms. |
| [adr/](adr/) | **Keep** ADR-0001/0002/0003 (framework decisions). Add new ADRs for *your* module's decisions, numbered onward. |

- [ ] Delete this page (`00-adapting-to-your-module.md`) once the module is adapted — it only
      matters at stamping time.

## The rule of thumb

Framework-level pages (philosophy, background, technology, maintainer guide, contributing, the
three seed ADRs) are **reusable verbatim** — they explain how *any* Backbone module works.
Entity-level pages (architecture's request trace, the developer guide's recipes, the glossary's
domain terms) are **yours to fill** as `Example` becomes your real entity.

---

Back to the [handbook index](../README.md).
