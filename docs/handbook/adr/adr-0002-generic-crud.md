# ADR-0002: CRUD is inherited from generics, not written per entity

- **Status:** Accepted
- **Date:** 2026-07-03
- **Deciders:** Backbone Framework maintainers

## Context

Even with generation ([ADR-0001](adr-0001-schema-yaml-ssot.md)), we must decide *what* gets
generated for the service and repository layers. One option is to generate a full, bespoke `impl`
of every CRUD method for every entity — hundreds of near-identical lines per entity, multiplied
across every module. That is a lot of generated code to compile, read, and keep behaviorally
consistent, and any fix to CRUD behavior would mean regenerating every entity everywhere.

## Decision

**Standard CRUD lives once, in the framework crates, and entities inherit it.**

- A repository is a **thin newtype** over `backbone_orm::GenericCrudRepository<Entity, SoftDelete>`,
  exposing all standard methods via `Deref`:
  ```rust
  pub struct ExampleRepository(GenericCrudRepository<Example, SoftDelete>);
  ```
- A service is a **type alias**, not an `impl`:
  ```rust
  pub type ExampleService =
      GenericCrudService<Example, CreateExampleDto, UpdateExampleDto, ExampleRepository>;
  ```
- The HTTP surface is one call to `BackboneCrudHandler::routes(service, "/collection")`, which
  yields all twelve endpoints.

Custom behavior is *added* (a method on the newtype, a `*_custom.rs` service), never a hand-rolled
replacement. If `GenericCrudRepository` cannot express a query, extend it with a custom method —
do not bypass it.

## Alternatives considered

- **Generate a full `impl` per entity.** Maximally explicit, but multiplies compile time and code
  volume, and a CRUD-behavior fix must be regenerated across every entity. Rejected.
- **A runtime base class / trait objects.** Loses the compile-time-checked query guarantee SQLx
  gives and adds dynamic dispatch on the hot path. Rejected.
- **Hand-rolled repositories and Axum routes per entity.** The status quo Backbone exists to
  replace; inconsistent and unbounded in effort. Rejected — and made an explicit anti-pattern.

## Consequences

**Easier:** every entity's CRUD is identical by construction; a fix or feature in
`GenericCrudService`/`GenericCrudRepository` reaches every module at once; generated modules stay
small and readable; adding an entity adds an alias and a newtype, not a wall of methods.

**Harder / to live with:** the generic types are load-bearing framework API — a breaking change to
them ripples everywhere (mitigated by pinning the `backbone-*` git deps to a tag/rev for releases);
genuinely non-CRUD access requires a custom method rather than an ad-hoc query, which some
developers reach for out of habit; the `Deref`-to-generic indirection is one more hop to understand
when reading a repository for the first time.
