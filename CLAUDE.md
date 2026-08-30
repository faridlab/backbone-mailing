# Metaphor Domain Module

> Type: **`module`** — a bounded-context library crate. 4-layer DDD. Schema YAML is the single source of truth; most code is regenerated.
> This file orients Claude. Skills carry depth; load them on demand.

## What this is

A library crate (no `main.rs`) that owns one business domain (e.g. `accounting`, `billing`, `crm`). Consumed by `backend-service` projects. Exposes a `{Domain}Module` struct built via `builder()` that wires all services. Twelve standard CRUD endpoints per entity are auto-wired via `BackboneCrudHandler` — you only write *custom* logic.

## Golden path

```bash
metaphor schema schema validate                 # check schema YAML
metaphor make entity <Name>              # scaffold from schema
metaphor migration generate <name>         # new migration
metaphor dev test                        # run tests
metaphor lint check
```

## The single source of truth

**`schema/models/<entity>.model.yaml`** defines every entity. From it, the codegen pipeline produces:
- domain entity struct
- DTOs (`Create`, `Update`, `Response`)
- SQL migration (in `migrations/`)
- repository newtype
- service type alias
- HTTP handler + route registration
- (optional) gRPC service + Protobuf
- (optional) OpenAPI spec

**Regeneration preserves only code inside `// <<< CUSTOM ... // END CUSTOM` blocks.** Everything outside those markers is overwritten.

## Rules

- **MUST** edit `schema/models/*.model.yaml` first for any entity change. Never hand-edit generated files outside CUSTOM markers.
- **MUST** put custom logic inside `// <<< CUSTOM` / `// END CUSTOM` blocks, or in a sibling `*_custom.rs` file (e.g. `account_service_custom.rs`) which is never overwritten.
- **MUST** define services as type aliases: `pub type AccountService = GenericCrudService<Account, CreateAccountDto, UpdateAccountDto, AccountRepository>`. Don't hand-roll `impl`.
- **MUST** define repositories as thin newtypes: `pub struct AccountRepository(GenericCrudRepository<Account, PgPool>)`. Add custom methods only when `GenericCrudRepository` cannot express them.
- **MUST** register every service in the `{Domain}Module` builder.
- **MUST** be a library — no `main.rs`, no binary target.
- **NEVER** write ad-hoc axum routes; use `BackboneCrudHandler` which gives all 12 endpoints (list / create / get / update / patch / soft_delete / restore / empty_trash / bulk_create / upsert / find_by_id / list_deleted).
- **NEVER** bypass `GenericCrudRepository` for simple CRUD — extend it via custom methods.
- **NEVER** touch another module's schema YAML.
- **MUST** read and follow the target repo's own `CLAUDE.md` when working across repos — before editing in another repo, read its rules; the more local `CLAUDE.md` always wins.

## Four-layer folder cheatsheet

```
src/
├── lib.rs                                # re-exports + {Domain}Module
├── module.rs                             # Module struct + builder
├── domain/
│   ├── entity/
│   │   ├── <entity>.rs                   # generated; customize via CUSTOM markers
│   │   └── mod.rs
│   └── repositories/                     # trait definitions (ports)
├── application/
│   ├── service/
│   │   ├── <entity>_service.rs           # type alias to GenericCrudService
│   │   └── <entity>_service_custom.rs    # custom methods (never regenerated)
│   └── dto/
├── infrastructure/
│   ├── persistence/
│   │   ├── <entity>_repository.rs        # newtype over GenericCrudRepository
│   │   └── mod.rs
│   ├── cache/                            # optional
│   ├── messaging/                        # optional
│   └── jobs/                             # optional
├── presentation/
│   ├── http/
│   │   ├── <entity>_handler.rs           # BackboneCrudHandler wiring
│   │   └── mod.rs
│   ├── dto/
│   ├── middleware/
│   └── grpc/                             # optional, feature-gated
└── routes/
    └── mod.rs                            # stateless + stateful composers

migrations/                               # NNN_description.up.sql / .down.sql
schema/
├── models/
│   └── <entity>.model.yaml               # ← SOURCE OF TRUTH
└── openapi/
    └── index.openapi.yaml                # generated
seeders/                                  # test data generators
config/                                   # optional module-local config
tests/                                    # integration tests

Cargo.toml                                # feature flags: events, grpc, openapi
```

## Tech stack (non-negotiable)

- Rust 2021; `[lib]` only.
- Web/RPC: Axum / Tonic (feature-gated).
- DB: SQLx 0.8 over PostgreSQL; queries are compile-time checked.
- Async: Tokio 1.x.
- Errors: `thiserror` for domain errors.

## Naming conventions

- Entities: PascalCase (`Account`, `Journal`).
- Tables: snake_case plural (`accounts`, `journals`).
- Services: `{Entity}Service`.
- Repositories: `{Entity}Repository`.
- DTOs: `Create{Entity}Dto`, `Update{Entity}Dto`, `{Entity}Response`.
- Handlers: `{entity}_handler.rs`.
- Route fns: `create_{entity}_routes()`.

## Common tasks

- "Add a new entity `Vendor`" → add `schema/models/vendor.model.yaml` → `metaphor make entity vendor` → migration/entity/service/handler/route wired automatically → register `vendor_service` in `module.rs`.
- "Add a custom business rule" → put it in `application/service/<entity>_service_custom.rs`, or inside `// <<< CUSTOM` markers in the generated service.
- "Add a non-CRUD endpoint" → add a handler fn in `presentation/http/`, register in `routes/mod.rs` (outside `BackboneCrudHandler` composition).
- "Change a column" → edit schema YAML, `metaphor migration generate <change>`, regenerate.

## Key files to read before editing

- `schema/models/*.model.yaml` — the source of truth; never skip.
- `src/module.rs` — how services wire together.
- `src/lib.rs` — public re-exports.
- `migrations/` — current DB shape.
- The nearest `*_custom.rs` file for the area you're touching.

## Deeper knowledge (load on demand)

- Skill: `backbone-schema-maintainer` — schema YAML DSL, generators, DDD invariants.
- Skill: `custom-logic-specialist` — writing custom logic that survives regeneration.
- Skill: `database-migration-specialist` — safe PostgreSQL migrations.
- Skill: `creative-domain-architect` — bounded-context design.
- Skill: `modules-orchestrator` — composing modules into a service.
- Skill: `api-and-interface-design` — when shaping non-CRUD endpoints.

## Anti-patterns

- Editing generated code outside CUSTOM markers (silently overwritten on next regen).
- Adding `main.rs` / binary target to a module (wrong project type).
- Hand-rolled axum CRUD routes (always use `BackboneCrudHandler`).
- Skipping schema YAML and writing entity + migration + handler by hand (breaks regen).
- Leaking one module's entity into another module's API.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- For codebase questions, first run `graphify query "<question>"` when graphify-out/graph.json exists. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
