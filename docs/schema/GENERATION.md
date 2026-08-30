# Code Generation Reference

> **Purpose**: This document is the canonical reference for the Backbone schema code generator. It lists every generation target, every CLI flag, and the rules for opt-in generators, custom code preservation, and the migration pipeline.
>
> **Source of truth**: [crates/backbone-schema/src/generators/mod.rs](../../crates/backbone-schema/src/generators/mod.rs) (`GenerationTarget` enum) and [crates/backbone-schema/src/commands/schema.rs](../../crates/backbone-schema/src/commands/schema.rs) (CLI definitions). If this doc disagrees with the source files, the source files win — open a PR to fix the doc.

## Table of Contents

1. [Quick Start](#quick-start)
2. [Generation Targets (37 total)](#generation-targets-37-total)
3. [Opt-In Generators](#opt-in-generators)
4. [Per-Module and Per-Model Filtering](#per-module-and-per-model-filtering)
5. [CLI Reference](#cli-reference)
6. [Migration Pipeline](#migration-pipeline)
7. [Custom Code Preservation](#custom-code-preservation)
8. [Recent Generator Changes (Phases 1-10)](#recent-generator-changes-phases-1-10)
9. [Verification & Troubleshooting](#verification--troubleshooting)

---

## Quick Start

```bash
# 1. Generate everything for a module
./target/debug/backbone-schema schema generate <module> --target all --force

# 2. Generate a single target
./target/debug/backbone-schema schema generate <module> --target sql

# 3. Generate multiple targets
./target/debug/backbone-schema schema generate <module> --target sql,rust,repository

# 4. Validate schema without generating
./target/debug/backbone-schema schema validate <module>

# 5. Show schema drift vs database
./target/debug/backbone-schema schema status <module>

# 6. Generate incremental migration from schema changes
./target/debug/backbone-schema schema migration <module>
```

The example modules in this repo are `bersihir`, `sapiens`, `bucket`, `corpus`. Replace `<module>` accordingly.

---

## Generation Targets (37 total)

The generator currently supports **37 targets** organised into four logical groups. Each target maps to one variant of the `GenerationTarget` enum and one file in `crates/backbone-schema/src/generators/`.

### Data Layer (5)

| Target | Aliases | Output | Source |
|--------|---------|--------|--------|
| `sql` | `migration`, `migrations` | PostgreSQL migration files (`migrations/*.sql`) | `sql.rs` |
| `rust` | — | Domain entity structs (`src/domain/entities/*.rs`) | `rust.rs` |
| `proto` | `protobuf` | Protocol Buffer definitions (`proto/*.proto`) | `proto.rs` |
| `repository` | `repo` | Repository implementations (`src/infrastructure/repository/*.rs`) | `repository.rs` |
| `repository-trait` | `repo-trait` | Repository trait definitions (`src/domain/repository/*.rs`) | `repository_trait.rs` |

### Business Logic (12)

| Target | Aliases | Output | Source |
|--------|---------|--------|--------|
| `service` | `services`, `svc` | Application services (`src/application/service/*.rs`) | `service.rs` |
| `domain-service` | `domain-svc` | Domain services with dependencies (`src/domain/service/*.rs`) | `domain_service.rs` |
| `usecase` | `interactor` | Use case structs (`src/application/usecase/*.rs`) | `usecase.rs` |
| `auth` | `authentication`, `authorization` | Auth guards & permission constants (`src/authorization/*.rs`) | `auth.rs` |
| `events` | `domain-events`, `messaging` | Domain event types (`src/domain/events/*.rs`) | `events.rs` |
| `state-machine` | `sm` | State machine implementations (`src/domain/state_machine/*.rs`) | `state_machine.rs` |
| `validator` | `validation` | Field, entity, async validators (`src/domain/validator/*.rs`) | `validator.rs` |
| `specification` | `spec` | Specification pattern types (`src/domain/specification/*.rs`) | `specification.rs` |
| `cqrs` | `command`, `query` | **Opt-in** — CQRS command/query handlers | `cqrs.rs` |
| `computed` | `virtual` | Computed-field implementations | `computed.rs` |
| `bulk-operations` | `batch` | Bulk insert/update helpers | `bulk_operations.rs` |
| `value-object` | `vo` | Typed IDs and value objects (`src/domain/value_objects/*.rs`) | `value_object.rs` |

### API Layer (5)

| Target | Aliases | Output | Source |
|--------|---------|--------|--------|
| `handler` | `handlers`, `rest` | Axum HTTP handlers (`src/presentation/http/handlers/*.rs`) | `handler.rs` |
| `grpc` | `tonic` | Tonic gRPC services (`src/presentation/grpc/*.rs`) | `grpc.rs` |
| `graphql` | `gql` | GraphQL resolvers (`src/presentation/graphql/*.rs`) | `graphql.rs` |
| `openapi` | `swagger` | OpenAPI 3 specs (`docs/openapi/*.yaml`) | `openapi.rs` |
| `dto` | `dtos`, `data-transfer` | Request/response DTOs (`src/presentation/dto/*.rs`) | `dto.rs` |

### Infrastructure & Framework Compliance (15)

| Target | Aliases | Output | Source |
|--------|---------|--------|--------|
| `trigger` | `triggers` | Database triggers / Rust trigger handlers | `trigger.rs` |
| `flow` | `workflow`, `saga` | Workflow / Saga orchestrator (`src/application/workflow/*.rs`) | `flow.rs` |
| `module` | `lib` | Module-level `lib.rs` with public API | `module.rs` |
| `config` | `settings` | Module configuration boilerplate | `config.rs` |
| `projection` | `read-model` | **Opt-in** — CQRS projections | `projection.rs` |
| `event-store` | `eventstore` | Event-sourcing event store | `event_store.rs` |
| `export` | `public-api` | Public API exports for cross-module use | `export.rs` |
| `integration` | `acl` | Anti-corruption-layer adapters | `integration.rs` |
| `event-subscription` | `subscription` | Cross-module event subscribers | `event_subscription.rs` |
| `versioning` | `api-versioning` | API versioning helpers | `versioning.rs` |
| `seeder` | `seed`, `seeds` | Seed data SQL (`migrations/seeds/*.sql`) | `seeder.rs` |
| `integration-test` | `tests` | Integration test scaffolding | `integration_test.rs` |
| `audit-triggers` | — | DB-level audit triggers | `audit_triggers.rs` |
| `app-state` | `appstate` | Module `AppState` struct (Phase compliance) | `app_state.rs` |
| `routes-composer` | — | `http_routes()` composer (Phase 9) | `routes_composer.rs` |
| `handlers-module` | — | Top-level handlers `mod.rs` | `handlers_module.rs` |

> **Counting note**: 5 + 12 + 5 + 15 = 37 targets. CQRS and Projection are off by default (see [Opt-In Generators](#opt-in-generators)). The README mentions "31 generators" historically — that count predates the GraphQL, framework-compliance, and bulk-operation additions.

---

## Opt-In Generators

Two targets are **disabled by default** because not every module needs them:

### CQRS / Projection

```yaml
# index.model.yaml
config:
  generators:
    cqrs: true       # opt in to both Cqrs and Projection
```

When `cqrs: false` (or absent), the generator skips the `Cqrs` and `Projection` targets even if you pass `--target all`. To force them via CLI, list them explicitly: `--target cqrs,projection`.

### Why Opt-In

CQRS adds command/query handler layers and read-model projections. For simple CRUD modules this is overhead with no benefit. Bersihir, sapiens, and corpus all run with `cqrs: false`. Enable it only when you need event-sourced read models, complex query optimization, or strict command/query separation.

---

## Per-Module and Per-Model Filtering

### Module-Level (`index.model.yaml`)

```yaml
config:
  generators:
    # Whitelist mode — only these targets generated:
    enabled: [sql, rust, repository, handler, service, module, config]

    # OR blacklist mode — these targets skipped:
    disabled: [graphql, grpc, proto, openapi]

    cqrs: false
```

`enabled` and `disabled` are mutually exclusive: if `enabled` is set, only listed targets pass through.

### Per-Entity Override

```yaml
# audit_log.model.yaml — internal entity, no public API
models:
  - name: AuditLog
    collection: audit_logs
    fields:
      # ...
    generators:
      disabled: [handler, grpc, graphql, openapi, dto]
```

Per-entity `generators:` takes precedence over module-level config. The filter is applied during generation, so unwanted files are never written.

### Implementation

Filtering logic lives in `filter_targets_by_config()` at [crates/backbone-schema/src/generators/mod.rs:727](../../crates/backbone-schema/src/generators/mod.rs#L727).

---

## CLI Reference

The CLI binary is `backbone-schema` and the schema sub-command tree is:

```
backbone-schema schema <action> [args]
```

### `schema generate`

Generate code for one module.

```bash
backbone-schema schema generate <module> [flags]
```

| Flag | Default | Purpose |
|------|---------|---------|
| `--target <list>` | `all` | Comma-separated targets, or `all` |
| `--output <dir>` | module dir | Override output directory |
| `--dry-run` | false | Print what would be written without writing |
| `--force` / `-f` | false | Overwrite existing generated files |
| `--split` | false | One file per entity (e.g. for OpenAPI) |
| `--changed` | false | Only generate for schemas changed in git since `--base` |
| `--base <ref>` | `HEAD` | Git base for change detection |
| `--validate` | false | Run `cargo check` after generation; fail on errors |
| `--models <list>` | — | Filter: only these entity names |
| `--hooks <list>` | — | Filter: only these hook names |
| `--workflows <list>` | — | Filter: only these workflow names |
| `--lenient` | false | Skip strict validation when filtering |

### `schema validate`

Validate schema files without generating code.

```bash
backbone-schema schema validate <module> [-w]
```

Checks: YAML syntax, type references, model relations, DDD entity-model bindings, value object types, domain service dependencies, authorization role/permission consistency. Add `-w` / `--warnings` to also surface non-fatal warnings.

### `schema migration`

Generate an incremental migration by diffing the schema against the live database.

```bash
backbone-schema schema migration <module> [flags]
```

| Flag | Purpose |
|------|---------|
| `--output <file>` | Write migration to file (default: `migrations/<timestamp>_<name>.sql`) |
| `--destructive` | Include `DROP` / `ALTER ... DROP COLUMN` statements |
| `--safe-only` | Skip destructive ops; emit only additive changes |
| `--preview` | Print the SQL without writing files |
| `--database-url <url>` | Override `DATABASE_URL` env var for introspection |

`--destructive` and `--safe-only` are mutually exclusive. If neither is passed, the CLI emits all safe operations and warns about pending destructive ones.

### `schema status`

Read-only check that compares the YAML definitions against the live DB and reports drift. Use this in CI to detect un-migrated schema changes.

```bash
backbone-schema schema status <module> [--database-url <url>]
```

### `schema diff`

Show diff between current schema and a previous git ref.

```bash
backbone-schema schema diff <module> [--base <ref>]
```

### `schema watch`

Re-run generation on schema file changes.

```bash
backbone-schema schema watch <module> [-t <targets>] [-o <dir>]
```

### `schema changed`

List schema files that changed since a git ref.

```bash
backbone-schema schema changed [<module>] [--base <ref>] [--outputs] [--targets]
```

### `schema parse`

Debug helper: parse a schema and dump the AST.

```bash
backbone-schema schema parse <path> [-f json|pretty]
```

---

## Migration Pipeline

The migration generator (`schema migration`) runs an incremental pipeline:

1. **Parse** — Load `*.model.yaml` files for the module.
2. **Introspect** — Read the live database schema via `DATABASE_URL`.
3. **Diff** — Compute additive vs. destructive changes.
4. **Safety analysis** — Classify each change as safe (CREATE, ADD COLUMN with default) or destructive (DROP, ALTER TYPE narrowing).
5. **Emit SQL** — Write a single migration file containing the safe changes; warn on destructive changes unless `--destructive` is passed.
6. **Custom block preservation** — Append a `-- <<< CUSTOM SEED DATA >>>` marker; everything below the marker is preserved across regenerations.

To run migrations after generation:

```bash
DATABASE_URL="postgresql://root:password@localhost:5432/bersihirdb" \
  ./target/debug/backbone migration run --module bersihir
```

---

## Custom Code Preservation

The framework supports incremental regeneration without losing handwritten code. There are two preservation mechanisms.

### Custom Block Markers (in `.rs` files)

```rust
// Generated entity file: src/domain/entities/order.rs

pub struct Order { /* generated */ }

impl Order { /* generated methods */ }

// <<< CUSTOM
//
// Anything between the open marker and the close marker is preserved on every regeneration.
//
impl Order {
    pub fn business_specific_helper(&self) -> bool {
        self.total > Decimal::ZERO
    }
}
// CUSTOM >>>
```

The markers work in **all `.rs` files** the generator emits, including `mod.rs`. The block content survives every `schema generate` run.

### Custom Files (`_custom` suffix convention)

For larger custom code units, create a separate file with the `_custom` suffix:

```
src/application/service/
├── order_service.rs              ← generated, regeneratable
└── order_service_custom.rs       ← handwritten, never touched
```

Wire the custom file into `mod.rs` inside a `// <<< CUSTOM` block:

```rust
// src/application/service/mod.rs

pub mod order_service;

// <<< CUSTOM
pub mod order_service_custom;
pub use order_service_custom::*;
// CUSTOM >>>
```

### Custom Routes

Generated handlers register their routes through a `routes_composer.rs` (Phase 9). Custom HTTP endpoints belong in `custom_routes.rs`:

```rust
// src/presentation/http/custom_routes.rs
pub fn custom_routes() -> Router<AppState> {
    Router::new().route("/api/v1/orders/bulk-import", post(bulk_import))
}
```

Wire it into the app's route table — never edit the generated `routes_composer.rs`.

### Seed Files

Generated seed SQL files use a similar marker:

```sql
-- migrations/seeds/0001_orders_seed.sql

INSERT INTO orders (...) VALUES (...);   -- generated

-- <<< CUSTOM SEED DATA >>>
INSERT INTO orders (...) VALUES (...);   -- handwritten, preserved on regen
```

Everything below `-- <<< CUSTOM SEED DATA >>>` survives every regeneration.

---

## Recent Generator Changes (Phases 1–10)

The generator has been substantially refactored. If you're reading older docs or older generated code, the differences below explain what's no longer there.

### Phase 1 — Trait Aliases

Generators now emit code that uses generic trait aliases from `backbone-core` instead of hand-rolled per-entity traits. Less duplication, easier to read.

### Phase 2 — State Machine Enforcement

The entity generator enforces state machine transitions at the type level. Illegal transitions become impossible to write — no more manual transition guards.

### Phase 3 — Service Simplification

`Service` generator no longer emits per-entity adapter structs. Services now wrap a single `GenericCrudService<E>` instance and only add custom methods on top.

### Phase 4 — Repository Generics

`Repository` and `RepositoryTrait` now compose around `GenericCrudRepository<E>`. The entity-specific repository file is small — usually just custom queries.

### Phase 5 — Workflow + Trigger Composition

`Flow` and `Trigger` generators emit code that composes via `backbone-core` generics. No per-entity adapter boilerplate.

### Phase 6 — Pagination Types in Core

`DomainPaginationParams` and `DomainPaginatedResult` were extracted to `backbone-core`. Generated code references these directly.

### Phase 7 — Child Entity Collapse

For modules with parent-child entity hierarchies, child entities are now collapsed into the parent's CRUD stack. See the `bucket` module's `Thumbnail`, `FileVersion`, `AccessLog` collapse.

### Phase 8 — `StateMachineBehavior` in Core

The `StateMachineBehavior` and `TransitionMeta` traits live in `backbone-core`. Generated entity code imports these instead of defining its own.

### Phase 9 — `http_routes()` Composition

Child entity CRUD stacks are now collapsed via a shared `http_routes()` helper. The Bersihir module saved 11 child entity route blocks this way.

### Phase 10 — Sub-Workflow Decomposition

Monolithic workflows are deprecated. Compose workflows from focused sub-workflows that chain via domain events. See [RULE_FORMAT_WORKFLOWS.md](./RULE_FORMAT_WORKFLOWS.md#sub-workflow-composition-recommended-pattern).

### Other Recent Additions

- **GraphQL generator** (`graphql`) — full schema + resolver generation.
- **Per-model generator filtering** (`enabled` / `disabled`) — see above.
- **Soft-delete auto metadata column** — when `soft_delete: true`, the SQL generator injects a `metadata` JSONB column automatically.
- **UUID & date casting in `partial_update`** — generated update DTOs handle type coercion.
- **Audit metadata excluded from Create/Update DTOs** — `@audit_metadata` fields are auto-excluded from request DTOs.
- **Custom block duplication fix** — the generator no longer duplicates custom blocks on regeneration.
- **JSONB index field resolution** — indexes on JSONB fields work correctly.
- **`@unique` index deduplication** — duplicate unique indexes are merged.
- **Typed IDs with `AsRef` / `Deref`** — value-object IDs work seamlessly with anything that takes `&str` or `&Uuid`.
- **`From<&str>` / `From<&String>`** — string-based value objects can be constructed from references.
- **Domain error variant generation from hook rules** — each hook rule's `code:` becomes a domain error variant.

---

## Verification & Troubleshooting

### After Generation

```bash
# 1. Compile
cargo check -p <module>

# 2. Run module tests
cargo test -p <module>

# 3. Validate schema/database alignment
./target/debug/backbone-schema schema status <module>
```

### Common Issues

| Symptom | Likely Cause | Fix |
|---------|--------------|-----|
| `error[E0432]: unresolved import` | Generator out of sync with `backbone-core` traits | Rebuild `backbone-core` first |
| Custom block lost on regeneration | Marker formatting wrong | Ensure exact `// <<< CUSTOM` and `// CUSTOM >>>` markers |
| Migration tries to drop a column you still need | Stale DB introspection | Run `schema status` to confirm; use `--safe-only` |
| `Cqrs` / `Projection` files not generated | CQRS opt-in is off | Set `config.generators.cqrs: true` in `index.model.yaml` |
| GraphQL files missing | Module has GraphQL in `disabled:` list | Remove from `disabled` |
| Unexpected file generated for one entity | Per-module config doesn't filter entity-level | Add `generators.disabled` on the entity itself |

See [ERROR_CODES.md](./ERROR_CODES.md) for the full validation error reference.
