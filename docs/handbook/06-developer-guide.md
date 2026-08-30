<!-- Reader: App developer · Mode: Tutorial → How-to -->
# Developer Guide

Get from an empty directory to a running module with your own entity and twelve working REST
endpoints. The tutorial part holds your hand once; the recipes assume you know your way around.

Commands here were run against `metaphor 0.2.0`. Where the top-level [README](../../README.md)
shows a `backbone-schema`/`backbone` command, use the `metaphor` form below — those are the ones
that work today.

## Prerequisites

- **Rust** (2021 edition toolchain) and **Cargo**.
- The **`metaphor`** CLI on your `PATH` (`metaphor --version` → `metaphor 0.2.0` or newer).
- A reachable **PostgreSQL** instance.

## Install — start a new module from the skeleton

```bash
# 1. Copy the skeleton to where your module should live.
cp -r backbone-module-skeleton my-billing-module
cd my-billing-module

# 2. Name your crate: edit Cargo.toml [package].name (e.g. "billing-module").
#    The backbone-* crates are GIT dependencies pinned to branch = "main" — no path
#    fix-up needed. For a release, pin them to a tag/rev instead (see Technology page).
```

> The README's "fix dependency paths" step is stale — the deps are git, not path. Leave them, or
> pin to a tag.

## Quickstart — prove the toolchain end to end

The smallest thing that runs is the shipped `Example` entity. Point at a database and exercise it.

```bash
# From the module directory:
export DATABASE_URL="postgresql://root:password@localhost:5432/skeletondb"

# 1. Validate the schema that ships with the skeleton.
metaphor schema schema validate

# 2. Apply the two shipped migrations (enums + examples table).
metaphor migration run

# 3. Run the module's tests.
metaphor dev test
```

Expected: validation passes, migrations report the `examples` table created, and the test run is
green (the skeleton ships a `skeleton_compiles` placeholder test so `dev test` always has something
to run).

To see the HTTP surface, compose the module into a service and `metaphor dev serve`, then:

```bash
curl -s -X POST localhost:8080/api/v1/examples \
  -H 'content-type: application/json' \
  -d '{"name":"first","status":"active"}'
# → 201 { "id": "…", "name": "first", "status": "active", "metadata": { "createdAt": "…" } }
```

Note the JSON is **camelCase** (`createdAt`) even though the Rust and SQL are snake_case — that is
the generated `#[serde(rename_all = "camelCase")]` at work.

## Make it yours — rename `Example` to your entity

```bash
# 1. Rename and edit the schema model.
git mv schema/models/example.model.yaml schema/models/invoice.model.yaml
#    Inside it: Example → Invoice, examples → invoices, ExampleStatus → InvoiceStatus,
#    and add your real fields.
#    Update schema/models/index.model.yaml `imports:` to point at invoice.model.yaml.

# 2. Validate, generate, migrate.
metaphor schema schema validate
metaphor schema schema generate --target all --force
metaphor migration generate Invoice mailing
metaphor migration run

# 3. Wire the service into src/module.rs (see the Maintainer Guide, step 6), then test.
metaphor dev test
```

(`mailing` is your module name — auto-detected from the current directory when omitted, but
passing it explicitly is clearer in scripts.)

## Key concepts

Five ideas carry you the rest of the way. One line each; the linked page explains *why*.

- **Schema YAML is the source of truth.** You edit [`schema/models/*.model.yaml`](../schema/RULE_FORMAT_MODELS.md);
  the entity, DTOs, migration, repository, service, handler, and routes are generated from it.
  ([Philosophy](01-philosophy.md).)
- **A module is a library, not a service.** It has no `main.rs`. A `backend-service` composes it
  via `Module::builder().with_database(pool).build()?` and mounts `module.http_routes()`.
  ([Architecture](04-architecture.md).)
- **Twelve endpoints come free per entity.** `BackboneCrudHandler` gives list / create / get /
  update / patch / soft_delete / restore / empty_trash / bulk_create / upsert / find_by_id /
  list_deleted, mounted under `/api/v1/<collection>`.
- **CRUD is inherited, not written.** `Service = GenericCrudService<…>` is a type alias;
  `Repository` is a newtype over `GenericCrudRepository`. You add methods, never a fresh `impl`.
  ([ADR-0002](adr/adr-0002-generic-crud.md).)
- **Custom code survives regeneration** if it sits in `// <<< CUSTOM` markers, `*_custom.rs` files,
  or a `user_owned` path. Anything else is overwritten by `generate --force`.
  ([ADR-0003](adr/adr-0003-custom-markers.md).)

## Recipes

### How do I add a second entity to a module?

Follow the golden path in the [Maintainer Guide → Adding a new entity](05-maintainer-guide.md#adding-a-new-entity-the-golden-path).
In short: add the `.model.yaml`, add it to `index.model.yaml` `imports:`, `validate`, `generate`,
`migration generate`, `migration run`, then register the service in `src/module.rs`.

### How do I add a business rule (e.g. "an invoice can't be voided once paid")?

Write it in a `*_custom.rs` service file, not in the generated service:

```rust
// application/service/invoice_service_custom.rs
impl InvoiceServiceCustom {
    pub async fn void(&self, id: &str) -> ServiceResult<Invoice> {
        let inv = self.inner.find_by_id(id).await?.ok_or(ServiceError::NotFound)?;
        if inv.status == InvoiceStatus::Paid {
            return Err(ServiceError::Validation("cannot void a paid invoice".into()));
        }
        // …
    }
}
```

Register it in `mod.rs` under a `// <<< CUSTOM` marker. See
[custom-logic-specialist](../schema/EXAMPLES.md) territory.

### How do I add a non-CRUD endpoint?

Don't edit the generated handler. Add a handler fn in a `*_custom.rs`, compose it in `routes/`
beside the `BackboneCrudHandler` merge, and protect the file with a `user_owned` glob. Full steps:
[Maintainer Guide → Adding a non-CRUD endpoint](05-maintainer-guide.md#adding-a-non-crud-endpoint).

### How do I reference a user (or another module's entity)?

By **logical foreign key**, declared in the schema — never by copying the table in. The skeleton
already does this for audit actors:

```yaml
# schema/models/index.model.yaml
external_imports:
  - module: sapiens
    types: [User]
# …
created_by:
  type: uuid?
  attributes: ["@foreign_key(sapiens.User.id)"]
```

### How do I seed sample data?

Edit the seeder in `src/seeders/`, then:

```bash
metaphor migration seed mailing          # run Rust seeders
metaphor migration generate-seeds mailing  # emit SQL seed files
```

## Configuration

Defaults live in [`config/application.yml`](../../config/application.yml); override per environment
and at runtime.

| Option | Default | When to change |
|--------|---------|----------------|
| `server.host` | `0.0.0.0` | Bind to a specific interface. |
| `server.port` | `8080` | Port conflicts / multi-service hosts. |
| `database.url` | `postgresql://root:password@localhost:5432/skeletondb` | **Always** in real deployments — override with the `DATABASE_URL` env var, which takes precedence. |
| `database.max_connections` | `10` | Tune to your Postgres pool budget. |
| `logging.level` | `info` | `debug`/`trace` when diagnosing; `warn` in noisy prod. |

Layered files: `application.yml` (base) → `application-dev.yml` / `application-prod.yml`
(overrides). `DATABASE_URL` in the environment always wins over the YAML.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `backbone-schema: command not found` | Following the stale README | Use `metaphor schema schema …`. `backbone-schema` is not a separate binary here. |
| `metaphor migration run` can't connect | `DATABASE_URL` unset or Postgres down | `export DATABASE_URL=postgresql://…`; confirm Postgres is reachable. |
| My custom method vanished after regen | Code sat outside a protected region | Move it inside a `// <<< CUSTOM` marker, a `*_custom.rs` file, or a `user_owned` glob ([Maintainer Guide](05-maintainer-guide.md#regen-safety--the-rules-that-keep-your-logic-alive)). |
| New endpoint returns 404 | Route not composed, or service not registered | Merge the route in `routes/`; register the service field in `src/module.rs`. |
| `type ExampleStatus not found` after adding an enum variant | Migration not regenerated / not applied | Regenerate, `metaphor migration generate`, `metaphor migration run`. |
| Schema change ignored | Edited generated Rust instead of the YAML | Revert the Rust, edit `schema/models/*.model.yaml`, regenerate. |
| JSON field names look wrong (`created_at` vs `createdAt`) | Expecting snake_case on the wire | DTOs are `camelCase` by design; snake_case is DB/Rust only. |

---

Next: [Contributing](07-contributing.md) to send a change back, or the
[Glossary](08-glossary.md) to pin down a term.
