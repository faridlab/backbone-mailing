<!-- Reader: Maintainer · Mode: Explanation -->
# Architecture

A Backbone module is a **library crate** that owns one bounded domain as four DDD layers. It does
not run on its own — a `backend-service` composes it, hands it a database pool, and mounts its
router. Everything in `src/` is either generated from the schema YAML or lives inside a regen-safe
custom region. This page shows the system top-down (C4), then traces one request through all four
layers.

## 1. Context

Who uses the module, and what it depends on.

```mermaid
C4Context
    title System Context — a Backbone module
    Person(dev, "App developer", "Edits the schema YAML, writes custom logic")
    System(module, "Backbone module (this crate)", "One bounded domain, 4-layer DDD, schema-generated")
    System_Ext(svc, "backend-service", "Composes modules, owns main(), installs tracing")
    System_Ext(pg, "PostgreSQL", "Owns the module's schema + tables")
    System_Ext(sapiens, "sapiens module", "Owns User identity")
    System_Ext(cli, "metaphor CLI", "Generates code + migrations from the schema")

    Rel(dev, module, "edits schema, writes custom code")
    Rel(dev, cli, "runs generate / migrate / test")
    Rel(cli, module, "writes generated source + migrations")
    Rel(svc, module, "builds Module, mounts http_routes()")
    Rel(module, pg, "SQLx, compile-time-checked")
    Rel(module, sapiens, "logical FK (created_by → sapiens.User.id)")
```

*What to notice: the module is a **dependency**, never an entrypoint. The `metaphor` CLI writes
into it; a service consumes it; identity comes from a **sibling module by logical reference**, not
a copied-in table.*

## 2. Containers

The runnable/deployable pieces and how they talk. The module compiles into the service binary;
there is no separate module process.

```mermaid
flowchart LR
    client[HTTP client] -->|REST /api/v1/examples| svc
    subgraph svc[backend-service process]
        router[Axum Router]
        subgraph mod[Backbone module - linked in]
            handler[BackboneCrudHandler]
            service[GenericCrudService]
            repo[GenericCrudRepository]
        end
        router --> handler --> service --> repo
    end
    repo -->|SQLx| pg[(PostgreSQL<br/>own schema)]
```

*What to notice: the module contributes a `Router` that the service **merges** — the same object
Axum uses everywhere. Nothing about the module is a special runtime; it is ordinary linked-in Rust.*

## 3. Components / modules — the DDD 4-layer shape

Dependencies point **inward only**. Domain depends on nothing.

```mermaid
flowchart TD
    P["Presentation<br/>presentation/http/example_handler.rs<br/>routes/example_routes.rs"]
    A["Application<br/>application/service/example_service.rs<br/>application/dto/example_dto.rs"]
    D["Domain<br/>domain/entity/example.rs<br/>domain/repositories/example_repository.rs"]
    I["Infrastructure<br/>infrastructure/persistence/example_repository_impl.rs"]

    P --> A
    A --> D
    I --> D
    P -. mounts .-> M["module.rs — Module + ModuleBuilder"]
```

| Layer | Directory | Holds (in the skeleton) | May depend on |
|-------|-----------|-------------------------|---------------|
| **Domain** | `src/domain/` | `Example` entity (+ `ExampleId`, builder, `apply_patch`, audit accessors), `ExampleStatus` enum, the `ExampleRepository` **trait** (port), `ExampleFilter` | nothing |
| **Application** | `src/application/` | `ExampleService` (type alias over `GenericCrudService`), the Create/Update/Patch/Response/Summary/List DTOs and their conversions, `ServiceError`/`ServiceResult` (re-exported from `backbone-core`) | domain |
| **Infrastructure** | `src/infrastructure/` | `ExampleRepository` newtype over `GenericCrudRepository<Example, SoftDelete>`, `impl_crud_repository!` | domain, application |
| **Presentation** | `src/presentation/`, `src/routes/` | `create_example_routes()` wiring `BackboneCrudHandler`, `ExampleError` → HTTP mapping | application |
| **Composition** | `src/module.rs`, `src/lib.rs` | `Module` / `ModuleBuilder`, public re-exports | all layers (it is the root) |

A subtlety worth internalizing: there are **two `ExampleRepository`s**. The domain layer defines a
`trait ExampleRepository` (the *port* — 20+ async methods). The infrastructure layer defines a
`struct ExampleRepository` (the *adapter* — a newtype that `Deref`s to `GenericCrudRepository`).
The port is the contract; the adapter is the Postgres implementation.

## 4. Data & control flow — `POST /api/v1/examples` end to end

Trace one create request, top to bottom and back.

```mermaid
sequenceDiagram
    actor Client
    participant H as BackboneCrudHandler
    participant S as ExampleService (GenericCrudService)
    participant R as ExampleRepository (newtype)
    participant G as GenericCrudRepository
    participant DB as PostgreSQL

    Client->>H: POST /api/v1/examples {name, status}
    Note over H: deserialize CreateExampleDto<br/>(camelCase), validate @length
    H->>S: create(dto)
    Note over S: FromCreateDto: CreateExampleDto → Example<br/>(uuid v4 id, default metadata)
    S->>R: save(&example)
    R->>G: Deref → save
    G->>DB: INSERT INTO examples (...)
    Note over DB: audit trigger sets<br/>metadata.created_at / updated_at
    DB-->>G: row
    G-->>S: Example
    S-->>H: Example
    Note over H: Example → ExampleResponseDto (From)
    H-->>Client: 201 { id, name, status, metadata }
```

*What to notice:* four layers, but **only the schema-declared shapes cross them** — `CreateExampleDto`
in, `Example` through the middle, `ExampleResponseDto` out. Every conversion (`From<CreateExampleDto>
for Example`, `From<Example> for ExampleResponseDto`) is generated. The `created_at`/`updated_at`
stamps are set by a **Postgres trigger** ([`002_create_example_table.up.sql`](../../migrations/002_create_example_table.up.sql)),
not by Rust — so audit timestamps hold even for writes that bypass the service.

### The twelve endpoints, for free

`create_example_routes()` calls `BackboneCrudHandler::routes(service, "/examples")`. That single
call wires **all twelve** endpoints; you write none of them:

`list` · `create` · `get` · `update` · `patch` · `soft_delete` · `restore` · `empty_trash` ·
`bulk_create` · `upsert` · `find_by_id` · `list_deleted`

`routes/example_routes.rs` nests them under `/api/v1`, so the create endpoint above is
`POST /api/v1/examples`.

## Where persistence semantics come from

- **Soft delete** is structural: `config.soft_delete: true` in [`index.model.yaml`](../../schema/models/index.model.yaml)
  → `GenericCrudRepository<Example, SoftDelete>` → `soft_delete`/`restore`/`empty_trash`/`list_deleted`
  operate on `metadata.deleted_at`, and a partial index on `(metadata->>'deleted_at')` keeps the
  live-row query fast.
- **Audit** (`config.audit: true`) → the `metadata` JSONB column carrying `created_at`, `updated_at`,
  `deleted_at`, `created_by`, `updated_by`, `deleted_by`. Timestamps are trigger-managed; the `*_by`
  actor fields are logical FKs to `sapiens.User.id`.
- **Own schema per module** → migrations emit `CREATE SCHEMA <module>` and qualify tables as
  `<module>.<table>`, so two modules never collide on a table name.

## Key decisions

- [ADR-0001](adr/adr-0001-schema-yaml-ssot.md) — schema YAML is the single source of truth.
- [ADR-0002](adr/adr-0002-generic-crud.md) — services/repositories are generic, inherited not written.
- [ADR-0003](adr/adr-0003-custom-markers.md) — regen-safety via CUSTOM markers and `user_owned`.

---

Next: [Maintainer Guide](05-maintainer-guide.md) — how to add a feature without breaking the machine.
