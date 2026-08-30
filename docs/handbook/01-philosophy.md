<!-- Reader: Evaluator · Mode: Explanation -->
# Philosophy & motivation

**A Backbone module is a bounded business domain whose plumbing is generated, not written.**
You describe *what* an entity is in one YAML file; the framework produces the entity struct,
DTOs, migration, repository, service, HTTP handler, and routes — twelve REST endpoints per
entity — from that description. You write only the logic that is genuinely yours.

## The problem

Every service that touches a database re-writes the same layer cake for every entity:

- a struct and its `FromRow` mapping,
- create / update / patch / response DTOs and the conversions between them,
- a migration and its rollback,
- a repository with list / get / save / soft-delete / restore / bulk / count,
- a service that orchestrates them,
- an HTTP handler with the twelve endpoints, error mapping, and pagination.

That is hundreds of lines per entity, none of it interesting, all of it a place for bugs and
drift. Worse, it is *inconsistent*: entity A paginates one way, entity B another; A soft-deletes,
B hard-deletes; A's error codes differ from B's. The interesting 5% — the actual business rules —
drowns in the boilerplate 95%.

## The worldview

Three convictions shape everything here.

1. **The schema is the single source of truth.** [`schema/models/<entity>.model.yaml`](../schema/RULE_FORMAT_MODELS.md)
   is authoritative. The entity, DTOs, migration, repository, handler, and routes are *downstream
   artifacts* — regenerated, never hand-maintained. If code and schema disagree, the schema is
   right and the code is stale. (See [ADR-0001](adr/adr-0001-schema-yaml-ssot.md).)

2. **Boilerplate is generic, so make it generic once.** Standard CRUD is not written per entity;
   it is *inherited* from `GenericCrudService` and `GenericCrudRepository` in the framework crates.
   A module's `ExampleService` is a **type alias**, not an `impl`. (See
   [ADR-0002](adr/adr-0002-generic-crud.md).)

3. **Hand-written code must survive regeneration.** Business logic you write must not be
   clobbered the next time you regenerate. Two mechanisms guarantee this: `// <<< CUSTOM … //
   END CUSTOM` markers inside generated files, and whole files the generator never touches
   (`*_custom.rs`, plus paths listed in `metaphor.codegen.yaml`'s `user_owned`). (See
   [ADR-0003](adr/adr-0003-custom-markers.md).)

The payoff: adding an entity is editing one YAML file and running one command. Consistency is
structural, not disciplined — every entity in every module gets the *same* twelve endpoints, the
*same* soft-delete semantics, the *same* pagination and error shape, because they all come from
the same generic code.

## The 4-layer discipline

A module is not a flat bag of code. It is Domain-Driven Design's four layers, and the dependency
arrows only point inward:

```
Presentation  →  Application  →  Domain  ←  Infrastructure
   (HTTP)          (services)     (entities)    (Postgres)
```

- **Domain** knows nothing about HTTP or SQL. Just the entity and its invariants.
- **Application** orchestrates use cases over the domain.
- **Infrastructure** adapts the domain to Postgres, cache, messaging.
- **Presentation** exposes the application over Axum (and optionally gRPC/GraphQL).

The generator lays code into the correct layer for you. The [Architecture](04-architecture.md)
page traces a request through all four.

## What a module deliberately does **not** do

Non-goals are as important as goals — they are why the skeleton stays small.

- **It is not a service.** A module is a **library crate** — `[lib]` only, no `main.rs`, no binary.
  It is *composed into* a `backend-service`; it never runs on its own. Adding a binary target is
  using the wrong project type.
- **It does not own cross-domain logic.** One module = one bounded context. It never reaches into
  another module's schema or entities. User identity, for example, is *referenced* from the
  `sapiens` module by logical foreign key, not copied in.
- **It does not ship every layer up front.** The skeleton includes only Domain, Application,
  Infrastructure/persistence, Presentation/http, routes, and seeders. Event sourcing, CQRS, cache,
  gRPC, GraphQL, state machines, workflows — all are *placeholders* you add when a real requirement
  demands them, not speculative scaffolding.
- **It does not invite hand-rolled CRUD.** Ad-hoc Axum routes and bespoke repositories are an
  anti-pattern here. If `GenericCrudRepository` cannot express something, you extend it with a
  custom method — you do not bypass it.

## When this is the wrong tool

Be honest with yourself before adopting:

- If your domain is **not entity/CRUD-shaped** — a pure computation engine, a streaming pipeline,
  a stateless transformer — the generated layer cake buys you little.
- If you are **not on PostgreSQL**, the migration and repository generators target Postgres
  specifically; another store means writing the infrastructure layer by hand.
- If you need **one throwaway endpoint**, a full DDD module is heavier than the task.

For everything that *is* a bounded domain of persistent entities behind an API — accounting,
billing, CRM, inventory — this is exactly the shape that pays off, and it pays off more with every
entity you add.

---

Next: [Background & prior art](02-background.md) — what came before and why it fell short.
