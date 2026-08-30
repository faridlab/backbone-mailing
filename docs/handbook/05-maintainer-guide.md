<!-- Reader: Maintainer · Mode: How-to -->
# Maintainer Guide

How to maintain this module and add features without breaking the regeneration machine. If you
only read one rule, read this one: **edit the schema YAML, then regenerate; put hand-written code
only where the generator promises not to touch it.**

All commands below were run against `metaphor 0.2.0`. Where the top-level README differs, this
guide has the working form.

## Before you touch anything

- Read this project's [`CLAUDE.md`](../../CLAUDE.md) and the workspace `metaphor.yaml`.
- Confirm the project type is **`module`** — that dictates every rule here. A module is a
  **library** (`[lib]` only). Never add a `main.rs` or a binary target.
- Internalize the source of truth: **`schema/models/<entity>.model.yaml`**. Code is downstream.

## Where code goes (and what it may depend on)

| Layer | Directory | Put here | May depend on |
|-------|-----------|----------|---------------|
| Domain | `src/domain/` | Entities, value objects, invariants, repository **traits** | nothing |
| Application | `src/application/` | Services (type aliases), DTOs, use cases | domain |
| Infrastructure | `src/infrastructure/` | Repository impls, cache, messaging, jobs | domain, application |
| Presentation | `src/presentation/`, `src/routes/` | HTTP/gRPC handlers, route composition | application |

Dependency arrows point inward. If you find the domain layer importing `axum` or `sqlx`, something
is in the wrong layer.

## The two things you actually write

Almost everything is generated. Your hand-written work is exactly two kinds:

1. **Services** (type aliases) and their **custom methods**.
2. **Custom (non-CRUD) endpoints** and their routes.

Everything else — entity struct, DTOs, migration, repository newtype, the twelve CRUD endpoints —
comes from the schema.

## Adding a new entity (the golden path)

Say you want a `Vendor`.

```bash
# 1. Describe it. Either scaffold a schema stub…
metaphor make entity Vendor --module mailing
#    …or copy schema/models/example.model.yaml → vendor.model.yaml and edit it,
#    then add `- vendor.model.yaml` under `imports:` in schema/models/index.model.yaml.

# 2. Validate the schema before generating.
metaphor schema schema validate mailing

# 3. Generate all artifacts (entity, DTOs, repo, service, handler, routes).
metaphor schema schema generate mailing --target all --force

# 4. Generate the migration for the new entity.
metaphor migration generate Vendor mailing

# 5. Apply migrations.
metaphor migration run

# 6. Register the service in the module composition root (see below), then:
metaphor dev test
```

> `mailing` is your module name (auto-detected from the current directory when omitted).
> `--target` accepts a comma-separated subset if you want to regenerate just part of the cake
> (e.g. `--target dto,handler`). Run `metaphor schema schema generate --help` for the 31 targets.
> Use `--dry-run` first if you want to see what would change without writing.

### Step 6 in detail — wire the service into `Module`

Generation does **not** edit the composition root for you. Open [`src/module.rs`](../../src/module.rs)
and follow the `Example` pattern exactly:

```rust
pub struct Module {
    pub example_service: Arc<ExampleService>,
    pub vendor_service:  Arc<VendorService>,   // ← add the field
}

// in ModuleBuilder::build():
let vendor_repository = Arc::new(VendorRepository::new(db_pool.clone()));
let vendor_service    = Arc::new(VendorService::with_repository(vendor_repository.clone()));
Ok(Module { example_service, vendor_service })   // ← return it

// in Module::http_routes():
Router::new()
    .merge(create_example_routes(self.example_service.clone()))
    .merge(create_vendor_routes(self.vendor_service.clone()))   // ← mount it
```

Then re-export it in [`src/lib.rs`](../../src/lib.rs) alongside `pub use application::service::ExampleService;`.

## Changing an existing entity

1. Edit the field in `schema/models/<entity>.model.yaml` (the SSoT — never the generated struct).
2. `metaphor schema schema validate mailing`.
3. Generate a migration for the change:
   `metaphor migration generate <Entity> mailing` (or, for a schema-diff-driven migration against
   a live DB, `metaphor schema schema migration mailing --database-url …`).
4. Regenerate code: `metaphor schema schema generate mailing --target all --force`.
5. `metaphor migration run && metaphor dev test`.

See [database-migration-specialist](../schema/GENERATION.md) territory and the schema docs for
migration safety (`--safe-only`, `--destructive`, `--preview`).

## Regen-safety — the rules that keep your logic alive

Regeneration **overwrites everything outside a protected region.** There are three protected
mechanisms; know which one you are using.

### 1. `// <<< CUSTOM … // END CUSTOM` markers (inside generated files)

The generator preserves whatever sits between the markers. You will find empty ones ready to fill:

```rust
// in application/service/example_service.rs
// <<< CUSTOM
// END CUSTOM
```

Marker spellings vary slightly by file — the entity uses `// <<< CUSTOM METHODS START >>>` /
`// <<< CUSTOM METHODS END >>>`, the DTO file uses `// <<< CUSTOM DTOs` / `// >>> END CUSTOM DTOs`.
**Match the spelling already in the file**; add your code between the existing pair, do not invent
new marker text.

Use markers for small additions: a helper method on the entity, an extra DTO, a re-export.

### 2. `*_custom.rs` sibling files (never generated, never overwritten)

For anything substantial, write a whole file the generator never emits and so never touches:

```rust
// application/service/example_service_custom.rs   ← the generator will never write this name
use std::sync::Arc;
use crate::application::service::ExampleService;

pub struct ExampleServiceCustom {
    inner: Arc<ExampleService>,
    // domain-specific deps
}
// … your business rules …
```

Register it from the surrounding `mod.rs` **inside a `// <<< CUSTOM` marker** so the `mod`
declaration survives regeneration too.

### 3. `user_owned` globs in `metaphor.codegen.yaml`

[`metaphor.codegen.yaml`](../../metaphor.codegen.yaml) lists paths the generator skips **wholesale**
— never reads, merges, or deletes. The skeleton already protects `tests/features/**` and `docs/**`.
Add your hand-authored service files and guarded routes here when you want a whole path immune to
generation:

```yaml
user_owned:
  - "src/application/service/onboarding_service.rs"
  - "src/presentation/http/guarded_routes.rs"
  - "tests/features/**"
  - "docs/**"
```

**Which to reach for:** a few lines → a CUSTOM marker; a cohesive unit of logic → a `*_custom.rs`
file; an entire hand-owned subtree → a `user_owned` glob.

## Adding a non-CRUD endpoint

The twelve CRUD endpoints come from `BackboneCrudHandler`. For anything else (a report, an action,
a search), do **not** touch the generated handler. Instead:

1. Write the handler fn in a new `presentation/http/*_custom.rs` (or a `custom_routes.rs`).
2. Compose it in `routes/` *alongside* — not inside — the `BackboneCrudHandler` merge.
3. Protect the file via `user_owned` if it lives under a generated tree.

Never hand-roll a route that duplicates a CRUD endpoint — extend, don't replace.

## Build, test, lint

```bash
metaphor dev test          # unit + integration + E2E for this module
metaphor lint check        # clippy + fmt policy
metaphor dev serve         # run the composing service locally (gRPC + REST)
```

Never run bare `cargo build`/`cargo test` from the workspace root — each project has its own
`Cargo.toml`; use the `metaphor` wrappers so workspace policy applies. Inside *this* module
directory, `cargo test` works but `metaphor dev test` is preferred.

## Versioning & release

- This crate is versioned in [`Cargo.toml`](../../Cargo.toml) (`0.1.3` today). Bump per
  conventional-commits: `fix:` → patch, `feat:` → minor, `feat!:`/`BREAKING CHANGE` → major.
- Before releasing: `metaphor dev test` and `metaphor lint check` clean.
- Pin the `backbone-*` git deps to a tag/rev for any release build (see [Technology](03-technology.md)).
- Commits use conventional commits and carry **no Claude / co-author signature** — see
  [Contributing](07-contributing.md).

## What will break things

- **Editing generated code outside a CUSTOM marker** — silently overwritten on the next
  `generate --force`. This is the number-one regression.
- **Adding `main.rs` / a binary target** — wrong project type; a module is a library.
- **Hand-rolled Axum CRUD** — always use `BackboneCrudHandler`.
- **Skipping the schema** and writing entity + migration + handler by hand — breaks regeneration
  forever after.
- **Touching a sibling module's schema** — one module owns one bounded context; reference other
  modules by logical FK, never edit theirs.

---

Next: [Developer Guide](06-developer-guide.md) if you are integrating a module rather than maintaining one.
