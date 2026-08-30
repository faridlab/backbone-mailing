# ADR-0001: Schema YAML is the single source of truth

- **Status:** Accepted
- **Date:** 2026-07-03
- **Deciders:** Backbone Framework maintainers

## Context

A domain module produces a large, repetitive artifact set per entity: a struct, create/update/
patch/response DTOs, a migration and rollback, a repository, a service, an HTTP handler, and route
registration. Kept by hand, these drift — the DTO says one thing, the migration another, the entity
a third — and the drift is silent until a request fails at runtime. We need *one* place that is
authoritative, from which the rest is derived, so consistency is structural rather than a matter of
discipline.

## Decision

**`schema/models/<entity>.model.yaml` is the single source of truth.** Every downstream artifact —
domain entity, DTOs, SQL migration, repository newtype, service alias, HTTP handler, route
registration, and optional proto/OpenAPI — is *generated* from it by `metaphor schema schema
generate`. Hand-editing a generated file (outside a protected region) is not a supported operation;
the next regeneration overwrites it. When code and schema disagree, the schema is correct and the
code is stale.

## Alternatives considered

- **Hand-written layers.** Total control, but does not scale past a few entities and drifts
  entity-to-entity. Rejected — the drift is exactly the problem.
- **Runtime ORM reflection** (derive shape from the DB or annotations at runtime). Hides the SQL,
  couples domain to persistence, and defeats compile-time checking. Rejected.
- **One-shot scaffolding** (generate once, then own the files). Consistent at birth only; erodes the
  moment two developers edit two entities. Rejected — generation must stay repeatable for the life
  of the module.
- **Code-as-source-of-truth** (write Rust, derive schema/migrations from it). Inverts the leverage:
  the boilerplate would still be hand-written. Rejected.

## Consequences

**Easier:** adding an entity is editing one YAML file and running one command; every entity gets
identical CRUD, pagination, soft-delete, and error semantics; reviews focus on the schema diff and
the small custom surface, not boilerplate.

**Harder / to live with:** contributors must resist editing generated Rust directly (the top
regression, and why [ADR-0003](adr-0003-custom-markers.md) exists); the schema DSL is a language to
learn (documented under [`docs/schema/`](../../schema/README.md)); a schema change means regenerate +
migrate, not a quick inline edit; the generator becomes critical infrastructure whose bugs affect
every module.
