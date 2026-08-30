<!-- Reader: Evaluator + Maintainer · Mode: Explanation -->
# Technology & the "why"

Every dependency in [`Cargo.toml`](../../Cargo.toml) earns its place. This page gives each
significant choice a one-line rationale and names the alternative that was rejected, so an
evaluator can judge the stack and a maintainer knows *why* not to swap a piece out casually.

The versions below are what this skeleton pins at **v0.1.3**; where behavior is version-specific,
the version is called out.

## The choices

| Layer | Choice | Why | Rejected alternative |
|-------|--------|-----|----------------------|
| Language | **Rust 2021**, `[lib]` only | Memory safety + a type system strong enough to make generated code *provably* consistent; no GC pauses in a service hot path | Go (weaker types for the generated-DTO story), Kotlin (already used for the mobile edge, not the domain core) |
| Async runtime | **Tokio 1.x** (`full`) | The de-facto async runtime; Axum and SQLx are both built on it, so there is one reactor | `async-std` (smaller ecosystem, no Axum/SQLx alignment) |
| HTTP | **Axum 0.7** (+ `tower`, `tower-http`) | Tower middleware ecosystem, first-class extractors, and it composes as a plain `Router` — exactly what `BackboneCrudHandler` returns and the module merges | `actix-web` (its own actor model fights the compose-a-Router design) |
| Database | **PostgreSQL** via **SQLx 0.8** | Queries are **checked at compile time** against the schema — the codegen's consistency guarantee extends all the way to SQL; native enum, `uuid`, `jsonb` support | Diesel (heavier macro layer, less async-native), an ORM with runtime-only query building |
| Domain errors | **`thiserror` 1.0** | Ergonomic, zero-cost typed errors for the domain/service layers; the generated handler maps them to HTTP status + a stable error code | `anyhow` for domain errors (loses the typed variants the handler matches on) |
| Boundary errors | **`anyhow` 1.0** | Right tool at the *composition* boundary (`ModuleBuilder::build` returns `anyhow::Result`) where a typed enum adds no value | `thiserror` everywhere (ceremony with no payoff at the boundary) |
| Serialization | **`serde` / `serde_json`** | Universal; DTOs derive `Serialize`/`Deserialize` and `#[serde(rename_all = "camelCase")]` gives a stable JSON wire shape | manual (de)serialization (error-prone, defeats codegen) |
| IDs / time / money | **`uuid` v4**, **`chrono`**, **`rust_decimal`** | UUID primary keys avoid enumeration and merge cleanly across modules; `chrono` for audit timestamps; `rust_decimal` shipped by default because any `decimal` schema field generates code that imports it | integer PKs (leak ordinality, collide across modules), `f64` money (rounding bugs) |
| Config | **`config` 0.14** + **`serde_yaml`** | Layered YAML (`application.yml` + env overrides) matches the `config/` convention; `DATABASE_URL` overrides at runtime | hardcoded config, bespoke env parsing |
| Validation | **`validator` 0.16** (feature-gated) | DTO field rules (`@length(max=200)` → `#[validate(length(max = 200))]`) are declared in the schema and enforced at the edge | hand-written guard clauses scattered across handlers |
| gRPC / proto | **`tonic` 0.12** + `buf.yaml` | Optional, feature-gated second transport generated from the same schema | REST-only (loses the schema-drives-two-transports property) |
| Logging | **`tracing`** (+ `tracing-subscriber`) | Structured, async-aware spans; the service host installs the subscriber | `log` (no span/async context) |

## The framework crates

Four crates carry the leverage. In this skeleton they are **git dependencies** on the public
framework repo, pinned to `branch = "main"`:

```toml
backbone-core      = { git = "https://github.com/faridlab/backbone-framework", branch = "main", features = ["postgres"] }
backbone-orm       = { git = "https://github.com/faridlab/backbone-framework", branch = "main" }
backbone-auth      = { git = "https://github.com/faridlab/backbone-framework", branch = "main" }
backbone-messaging = { git = "https://github.com/faridlab/backbone-framework", branch = "main" }
```

| Crate | Gives the module | Seen in the skeleton as |
|-------|------------------|-------------------------|
| **`backbone-core`** | `GenericCrudService`, `BackboneCrudHandler`, `PersistentEntity`, `FromCreateDto` / `ApplyUpdateDto`, `ServiceError` / `ServiceResult` | the service type alias, the handler, DTO conversions, `service/error.rs` |
| **`backbone-orm`** | `GenericCrudRepository`, `SoftDelete`, `EntityRepoMeta`, pagination types | the repository newtype, the entity's `EntityRepoMeta` impl |
| **`backbone-auth`** | identity / permission primitives | reserved for the `permission/` and `auth/` layers |
| **`backbone-messaging`** | message-bus adapters | reserved for the `messaging/` layer |

> **Reproducibility note.** `branch = "main"` is convenient but *not reproducible* — a fresh
> `cargo build` can pull a newer commit. For anything you ship, pin to a tag or commit:
> `tag = "vX.Y.Z"` or `rev = "<sha>"`. `Cargo.lock` is committed, which pins transitively, but the
> git ref is what a `cargo update` will move.

> ⚠️ **Doc drift flagged.** The top-level [README](../../README.md) step 2 calls the `backbone-*`
> crates "path dependencies … [that] must point at your actual checkout." They are **git
> dependencies today**, not path deps — the `Cargo.toml` comment even notes this lets the skeleton
> "work anywhere on disk without path fix-up." Follow the `Cargo.toml`, not that README step.

## The CLI: `metaphor`, not `backbone-schema`

Generation, migration, and testing go through the **`metaphor`** binary (v0.2.0 at time of
writing), which dispatches to plugins (`metaphor-schema`, `metaphor-codegen`, `metaphor-dev`).

> ⚠️ **Doc drift flagged.** The top-level README invokes a standalone `backbone-schema` binary and
> `backbone migration run`. Those are **stale** — `backbone-schema` is not on `PATH`. The working
> forms are `metaphor schema schema generate …` and `metaphor migration run`. The
> [Developer Guide](06-developer-guide.md) and [Maintainer Guide](05-maintainer-guide.md) use the
> verified commands throughout.

Why a workspace CLI instead of raw `cargo`/`sqlx`? Because a module never lives alone — it is one
project in a multi-project workspace, and `metaphor` applies workspace-wide policy (affected-only
builds, cross-project codegen, plugin discovery). See [ADR-0002](adr/adr-0002-generic-crud.md) for
the generic-CRUD decision and the schema docs' [INTEGRATION](../schema/INTEGRATION.md) for how the
pieces compose.

---

Next: [Architecture](04-architecture.md) — the C4 view and a request traced end-to-end.
