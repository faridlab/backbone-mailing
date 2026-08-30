<!-- Reader: All · Mode: Reference -->
# Glossary — ubiquitous language

One term, one meaning, used everywhere in this handbook and in the code. When a term here names a
type or file, that name is exact. If you find a doc using a different word for one of these, the doc
is the bug.

### Aggregate / Entity
A domain object with identity and a lifecycle, defined by one `schema/models/<name>.model.yaml`.
In the skeleton: `Example`. Generated into `src/domain/entity/<name>.rs` with a strongly-typed id
(`ExampleId`), a builder, `apply_patch`, and audit accessors.

### Application layer
The use-case layer (`src/application/`): services and DTOs. Depends on the domain; knows nothing
about HTTP or SQL.

### Audit metadata
The `metadata` JSONB field (`created_at`, `updated_at`, `deleted_at`, `created_by`, `updated_by`,
`deleted_by`) added when `config.audit: true`. Timestamps are set by a Postgres trigger; the `*_by`
actor fields are logical FKs to `sapiens.User.id`.

### `BackboneCrudHandler`
The `backbone-core` type that produces an Axum `Router` with all **twelve** CRUD endpoints for an
entity. Invoked as `BackboneCrudHandler::<…>::routes(service, "/collection")`. You never hand-write
these routes.

### Bounded context
The single business domain a module owns. One module = one bounded context. A module never edits
another's schema; it references other modules by logical FK.

### Composition root
`src/module.rs` — the `Module` struct and `ModuleBuilder`. Wires each service to its repository and
composes the routers. The one place that is allowed to depend on every layer.

### CUSTOM marker
A `// <<< CUSTOM … // END CUSTOM` region inside a generated file. Content between the markers
survives regeneration. Spelling varies per file (`// <<< CUSTOM METHODS START >>>`, `// <<< CUSTOM
DTOs`, …) — match what is already there.

### DTO (Data Transfer Object)
A wire-shape struct in `src/application/dto/`. Per entity: `Create…Dto`, `Update…Dto`, `Patch…Dto`,
`…ResponseDto`, `…SummaryDto`, `…ListResponseDto`. Serialized `camelCase`. Generated, with
`From`/`Apply` conversions to and from the entity.

### Domain layer
The innermost layer (`src/domain/`): entities, value objects, enums, invariants, and repository
**traits** (ports). Depends on nothing.

### Generation targets
The 31 kinds of artifact `metaphor schema schema generate` can emit (`rust`, `sql`, `dto`,
`handler`, `repository`, `service`, `proto`, `openapi`, …). `--target all` (default) emits the lot;
a comma-separated subset emits part.

### `GenericCrudRepository` / `GenericCrudService`
The `backbone-orm` / `backbone-core` generics that carry all standard CRUD. A module's repository is
a **newtype** over `GenericCrudRepository<Entity, SoftDelete>`; its service is a **type alias** over
`GenericCrudService<Entity, CreateDto, UpdateDto, Repository>`. Inherited, never re-implemented.

### Infrastructure layer
The adapter layer (`src/infrastructure/`): repository implementations, cache, messaging, jobs.
Depends on domain and application.

### Logical foreign key
A cross-module reference declared with `@foreign_key(module.Type.field)` (e.g.
`@foreign_key(sapiens.User.id)`). It documents the relationship and is *not* enforced by a database
constraint, so modules stay independently deployable.

### `metaphor`
The workspace CLI (v0.2.0) that orchestrates the projects and dispatches to plugins
(`metaphor-schema`, `metaphor-codegen`, `metaphor-dev`). Prefer it over raw `cargo`/`sqlx`. Note:
the standalone `backbone-schema` binary the README mentions is **not** installed; use `metaphor
schema schema …`.

### Module
A **library crate** owning one bounded context in 4-layer DDD, schema-driven. `[lib]` only — no
`main.rs`. Composed into a `backend-service`; never run alone. This repo is the *skeleton* for one.

### Own schema (per module)
Each module gets its own Postgres schema (`schema: mailing` in `index.model.yaml`). Migrations
`CREATE SCHEMA <module>` and qualify tables as `<module>.<table>`, so modules never collide on a
table name.

### Port / Adapter
The DDD names for the two `ExampleRepository`s: the **port** is the domain-layer `trait`
(the contract); the **adapter** is the infrastructure-layer `struct` (the Postgres implementation).

### Presentation layer
The transport layer (`src/presentation/`, `src/routes/`): HTTP handlers, route composition, and
optionally gRPC/GraphQL. Depends on the application layer.

### Regeneration (regen)
Re-running `metaphor schema schema generate … --force` to rebuild all downstream code from the
schema. Overwrites everything **outside** a protected region (CUSTOM markers, `*_custom.rs`,
`user_owned` globs).

### Schema (the SSoT)
`schema/models/*.model.yaml` — the single source of truth. Every entity struct, DTO, migration,
repository, service, handler, and route is generated from it. Not to be confused with the *Postgres
schema* (the per-module namespace).

### Soft delete
Marking a row deleted (`metadata.deleted_at` set) instead of removing it, enabled by
`config.soft_delete: true`. Backs the `soft_delete` / `restore` / `empty_trash` / `list_deleted`
endpoints.

### Twelve endpoints
The standard CRUD surface every entity gets from `BackboneCrudHandler`: `list`, `create`, `get`,
`update`, `patch`, `soft_delete`, `restore`, `empty_trash`, `bulk_create`, `upsert`, `find_by_id`,
`list_deleted`.

### `user_owned`
The `metaphor.codegen.yaml` key listing glob paths the generator skips wholesale — never reads,
merges, or deletes. The skeleton protects `tests/features/**` and `docs/**` (this handbook lives
under one of them).
