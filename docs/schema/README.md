# Backbone Schema System

> **Single source of truth** for your domain. You write three kinds of YAML files (`*.model.yaml`, `*.hook.yaml`, `*.workflow.yaml`); the framework generates everything else — entities, repositories, services, handlers, migrations, DTOs, OpenAPI specs, and more.
>
> This documentation set is also designed to be **copied into `backbone-module` as a standalone reference** so AI tools (Claude, Cursor, etc.) can use it when scaffolding new modules.

## Documentation Map

The schema docs are organised into three tiers. Read them in this order.

### Tier 1 — Authoring Rules (read these first)

These three documents are the canonical reference for the YAML files you write. They are kept in lock-step with the parser at [crates/backbone-schema/src/parser/yaml_parser/types.rs](../../crates/backbone-schema/src/parser/yaml_parser/types.rs).

| Document | What it covers |
|----------|----------------|
| [RULE_FORMAT_MODELS.md](./RULE_FORMAT_MODELS.md) | `*.model.yaml` — entity fields, types, attributes, relations, indexes, enums, shared types, soft-delete, per-model generator filtering |
| [RULE_FORMAT_HOOKS.md](./RULE_FORMAT_HOOKS.md) | `*.hook.yaml` — state machines, validation rules, RBAC + ABAC permissions, triggers, computed fields |
| [RULE_FORMAT_WORKFLOWS.md](./RULE_FORMAT_WORKFLOWS.md) | `*.workflow.yaml` — Saga steps, conditions, loops, parallel branches, sub-workflow chains, compensation |

### Tier 2 — Reference

| Document | What it covers |
|----------|----------------|
| [TYPES.md](./TYPES.md) | Full type system: primitives, composition, value objects, typed IDs, JSONB embedding |
| [GENERATION.md](./GENERATION.md) | All 37 generation targets, opt-in flags, CLI commands, migration pipeline, custom code preservation |
| [ARCHITECTURE.md](./ARCHITECTURE.md) | Generated DDD layers (consolidated) and file layout |
| [INTEGRATION.md](./INTEGRATION.md) | Cross-module foreign keys, event subscriptions, ACL adapters |
| [OPENAPI.md](./OPENAPI.md) | Optional `*.openapi.yaml` customisations |
| [SCHEMA-STANDARDS.md](./SCHEMA-STANDARDS.md) | Naming conventions, required fields, common patterns |
| [ERROR_CODES.md](./ERROR_CODES.md) | Validation error reference and troubleshooting |

### Tier 3 — Examples

| Document | What it covers |
|----------|----------------|
| [EXAMPLES.md](./EXAMPLES.md) | Real-world examples copied from the `bersihir` and `sapiens` modules — module index, simple entity, complex hook, sub-workflow, cross-module FK, per-entity generator filtering |

## Quick Start

### 1. Define Your Model

```yaml
# libs/modules/orders/schema/models/order.model.yaml

models:
  - name: Order
    collection: orders

    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]

      customer_id:
        type: uuid
        attributes: ["@required"]

      status:
        type: OrderStatus
        attributes: ["@default(draft)"]

      total:
        type: decimal
        attributes: ["@precision(10, 2)"]

      created_at:
        type: datetime
        attributes: ["@default(now)"]

    relations:
      customer:
        type: sapiens.User
        attributes: ["@one", "@foreign_key(customer_id)"]

      items:
        type: OrderItem[]
        attributes: ["@one_to_many"]

enums:
  - name: OrderStatus
    values:
      - draft
      - pending
      - confirmed
      - shipped
      - delivered
      - cancelled
```

### 2. Define Entity Hook

```yaml
# libs/modules/orders/schema/hooks/order.hook.yaml

name: Order
model: order.model.yaml

states:
  field: status

  values:
    draft:
      initial: true
    pending: {}
    confirmed: {}
    shipped: {}
    delivered:
      final: true
    cancelled:
      final: true

  transitions:
    submit:
      from: draft
      to: pending
      roles: [customer]

    confirm:
      from: pending
      to: confirmed
      roles: [admin, seller]

    ship:
      from: confirmed
      to: shipped
      roles: [seller]

    deliver:
      from: shipped
      to: delivered
      roles: [system]

    cancel:
      from: [draft, pending]
      to: cancelled
      roles: [customer, admin]

rules:
  minimum_order:
    condition: "total >= 10.00"
    message: "Minimum order is $10"
    code: ORDER_MINIMUM
```

### 3. Generate Everything

```bash
# Generate all code from schemas (37 generation targets)
./target/debug/backbone-schema schema generate orders --target all --force

# Output:
# ✓ Proto files generated
# ✓ Rust structs generated
# ✓ SQL migrations generated
# ✓ Repository implementations generated
# ✓ Repository traits generated
# ✓ Services generated
# ✓ Domain services generated
# ✓ Use cases generated
# ✓ State machines generated
# ✓ Domain events generated
# ✓ Validators generated
# ✓ Permissions generated
# ✓ Specifications generated
# ✓ CQRS implementations generated
# ✓ Computed fields generated
# ✓ HTTP handlers generated
# ✓ gRPC services generated
# ✓ OpenAPI specs generated
# ✓ Triggers generated
# ✓ Workflows generated
# ✓ Module code generated
# ✓ Config generated
# ✓ Value objects generated
# ✓ Projections generated
# ✓ Event store generated
# ✓ Exports generated
# ✓ Integration adapters generated
# ✓ Event subscriptions generated
# ✓ DTOs generated
# ✓ API versioning generated
```

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Business Requirements (BRD, Domain Docs)                    │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│  Schema Definitions (YAML)                                   │
│  ┌─────────────────────┐  ┌─────────────────────┐           │
│  │  *.model.yaml       │  │  *.hook.yaml        │           │
│  │  - Entities         │  │  - State machines   │           │
│  │  - Relations        │  │  - Validation rules │           │
│  │  - Indexes          │  │  - Permissions      │           │
│  │  - Enums            │  │  - Triggers         │           │
│  └─────────────────────┘  │  - Computed fields  │           │
│                           └─────────────────────┘           │
│  ┌─────────────────────┐                                    │
│  │  *.workflow.yaml    │                                    │
│  │  - Multi-step flows │                                    │
│  │  - Saga patterns    │                                    │
│  └─────────────────────┘                                    │
└─────────────────────────────────────────────────────────────┘
                              ↓ generate (37 targets)
┌─────────────────────────────────────────────────────────────┐
│  Generated Outputs (37 Targets)                           │
│                                                              │
│  Data Layer:                                                 │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │  Proto   │ │   Rust   │ │   SQL    │ │   Repo   │        │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘        │
│                                                              │
│  Business Logic:                                             │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │ Services │ │ UseCases │ │  Events  │ │  CQRS    │        │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘        │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │Validators│ │  Auth    │ │StateMach │ │  Perms   │        │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘        │
│                                                              │
│  API Layer:                                                  │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │ Handlers │ │   gRPC   │ │ OpenAPI  │ │   DTOs   │        │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘        │
│                                                              │
│  Infrastructure:                                             │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │
│  │Projection│ │EventStore│ │ Exports  │ │Versioning│        │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘        │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│  Working Application                                         │
│  - 11 Backbone CRUD endpoints + state transitions           │
│  - gRPC services with streaming support                     │
│  - Full validation and permission checks                    │
│  - CQRS with projections and event sourcing                 │
│  - API versioning (URL, header, query param)                │
└─────────────────────────────────────────────────────────────┘
```

## Schema Files You Write

You write **3 types of schema files** (+ optional OpenAPI customizations). All other code (31 generators) is **derived automatically** from these files.

```
libs/modules/{module}/schema/
├── models/                         # ← YOU WRITE: Entity definitions
│   ├── index.model.yaml            #   Module config, shared types, imports
│   ├── user.model.yaml             #   Entity: User
│   ├── role.model.yaml             #   Entity: Role
│   └── ...
│
├── hooks/                          # ← YOU WRITE: Entity lifecycle behaviors
│   ├── index.hook.yaml             #   Module events, scheduled jobs
│   ├── user.hook.yaml              #   Lifecycle: User state machine, rules, triggers
│   ├── role.hook.yaml              #   Lifecycle: Role permissions, validation
│   └── ...
│
├── workflows/                      # ← YOU WRITE: Multi-step business processes
│   ├── user_registration.workflow.yaml   # Saga: User registration flow
│   ├── password_reset.workflow.yaml      # Saga: Password reset flow
│   └── ...
│
└── openapi/                        # ← OPTIONAL: API customizations
    ├── index.openapi.yaml          #   Module-level OpenAPI config
    └── user.openapi.yaml           #   Per-entity API overrides
```

### What Gets Generated From What

| You Write | File Location | Generates (37 targets) |
|-----------|---------------|------------------------|
| **Models** | `schema/models/*.model.yaml` | Rust structs, SQL migrations, Proto, Repositories, Repository traits, DTOs, Handlers, gRPC, OpenAPI, Services, Use Cases, Validators, Specifications, Value Objects, Computed Fields, Projections, Event Store, Exports |
| **Hooks** | `schema/hooks/*.hook.yaml` | State Machines, Events, Triggers, Permissions, Auth guards, Domain Services, CQRS commands/queries |
| **Workflows** | `schema/workflows/*.workflow.yaml` | Workflow orchestration, Event Subscriptions, Integration adapters |
| **OpenAPI** (optional) | `schema/openapi/*.openapi.yaml` | OpenAPI customizations, API Versioning |

### Example: Creating a New Entity

1. **Create the model file:**
   ```bash
   # libs/modules/orders/schema/models/order.model.yaml
   ```

2. **Create the hook file:**
   ```bash
   # libs/modules/orders/schema/hooks/order.hook.yaml
   ```

3. **Generate all code:**
   ```bash
   backbone schema generate orders
   # Generates 37 targets automatically
   ```

> **Note:** You do NOT write separate files for use cases, projections, events, repositories, etc. These are all **derived automatically** from your model and hook definitions.

## Schema Format

The schema system uses **YAML format** for all definitions, providing:

- **Syntax highlighting** in all major IDEs
- **Schema validation** with JSON Schema
- **Familiar syntax** for developers and non-developers alike
- **Comments support** for documentation

### Model Files (`*.model.yaml`)

Define entities, fields, relations, indexes, and enums:

```yaml
models:
  - name: User
    collection: users
    extends: [Metadata]     # Inherit fields from shared types
    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]
      email:
        type: email
        attributes: ["@unique", "@required"]
      settings:
        type: UserSettings  # Shared type as JSONB column
    relations:
      roles:
        type: Role[]
        attributes: ["@many_to_many"]

enums:
  - name: UserStatus
    values: [active, inactive, suspended]
```

**Key features:**
- **`extends`**: Inherit fields from shared types as direct columns
- **Shared types as fields**: Use type names to store as JSONB
- **Type composition**: Combine types with `Metadata: [Timestamps, Actors]`

### Hook Files (`*.hook.yaml`)

Define entity lifecycle behaviors: state machines, validation rules, permissions, triggers, and computed fields:

```yaml
name: User
model: user.model.yaml

states:
  field: status
  values:
    active:
      initial: true
    suspended: {}

rules:
  unique_email:
    condition: "count(User, email == $this.email) == 0"
    message: "Email already exists"

permissions:
  admin:
    allow: [all]
  user:
    allow:
      - action: read
        if: "id == $actor.id"

computed:
  full_name: "[first_name, last_name].join(' ')"
```

### Workflow Files (`*.workflow.yaml`)

Define multi-step business processes (Saga pattern) that span multiple entities:

```yaml
name: UserRegistration
description: Complete user registration process

trigger:
  event: UserCreatedEvent

steps:
  - name: send_verification_email
    type: action
    action: send_email
    params:
      template: verification_email
      to: "{{ user.email }}"

  - name: wait_for_verification
    type: wait
    wait_for:
      event: EmailVerifiedEvent
      timeout: 24h

  - name: create_profile
    type: action
    action: create
    entity: Profile
    params:
      user_id: "{{ user.id }}"

  - name: activate_user
    type: transition
    entity: User
    transition: verify

compensation:
  - condition: "profile_created == true"
    action: delete
    entity: Profile
```

### OpenAPI Customization (`*.openapi.yaml`)

Override or extend generated OpenAPI specs:

```yaml
overrides:
  paths:
    /api/v1/users:
      get:
        parameters:
          - name: include_deleted
            in: query
            schema:
              type: boolean

custom_paths:
  /api/v1/users/me:
    get:
      operationId: getCurrentUser
      summary: Get current authenticated user
```

## When to Use Hook vs Workflow

| Use Case | Hook (`*.hook.yaml`) | Workflow (`*.workflow.yaml`) |
|----------|---------------------|------------------------------|
| Immediate side effects | ✅ | ❌ |
| Single entity operations | ✅ | ❌ |
| State machine transitions | ✅ | ❌ |
| Multi-step with waiting | ❌ | ✅ |
| Human approval chains | ❌ | ✅ |
| Timeout/expiration logic | ❌ | ✅ |
| Compensation/rollback | ❌ | ✅ |
| Cross-entity processes | ❌ | ✅ |

## Design Principles

1. **Single Source of Truth** - Schema defines everything, code is generated
2. **Business Readable** - YAML syntax that non-developers can review
3. **Developer Friendly** - Full IDE support with syntax highlighting
4. **Fully Generative** - Minimal manual code, maximum automation
5. **Type Safe** - Strong typing from schema to database to API
6. **Module Bounded** - Each module owns its schema (DDD bounded contexts)

## Recent Generator Changes (Phases 1-10)

The generator was refactored across ten phases. Older docs may describe patterns that no longer exist. Highlights:

- **Phase 1-4**: Per-entity adapter structs eliminated. Services and repositories now wrap `GenericCrudService<E>` / `GenericCrudRepository<E>` from `backbone-core`. Legacy `usecase/` directories (174 of them) were removed.
- **Phase 5**: Workflow + trigger composition via `backbone-core` generics — no per-entity workflow adapters.
- **Phase 8**: `StateMachineBehavior` and `TransitionMeta` extracted to `backbone-core`. Generated entities import these instead of redefining them.
- **Phase 9**: Child-entity CRUD stacks collapsed via a shared `http_routes()` helper.
- **Phase 10**: Monolithic workflows are deprecated. Decompose into focused sub-workflows that chain via domain events.
- **New capabilities**: GraphQL generator, per-model generator filtering (`enabled` / `disabled`), CQRS / Projection opt-in via `cqrs:` flag, soft-delete auto-injecting metadata column, custom code preservation in `mod.rs` and all `.rs` files, incremental migration CLI with `--destructive` and `--safe-only` flags.

For the full per-phase rundown, see [GENERATION.md → Recent Generator Changes](./GENERATION.md#recent-generator-changes-phases-1-10) and [ARCHITECTURE.md → Phase 1-10 Simplification Highlights](./ARCHITECTURE.md#phase-110-simplification-highlights).

## Next Steps

1. **Authoring rules** (read first):
   - [RULE_FORMAT_MODELS.md](./RULE_FORMAT_MODELS.md) — `*.model.yaml` syntax
   - [RULE_FORMAT_HOOKS.md](./RULE_FORMAT_HOOKS.md) — `*.hook.yaml` syntax
   - [RULE_FORMAT_WORKFLOWS.md](./RULE_FORMAT_WORKFLOWS.md) — `*.workflow.yaml` syntax
2. **Real examples**: [EXAMPLES.md](./EXAMPLES.md) (copied from `bersihir` / `sapiens`)
3. **Generator reference**: [GENERATION.md](./GENERATION.md) for targets, CLI flags, custom code preservation
4. **Type system deep-dive**: [TYPES.md](./TYPES.md) for shared types, value objects, typed IDs
5. **Layer overview**: [ARCHITECTURE.md](./ARCHITECTURE.md) to see what gets generated and where
6. **Cross-module integration**: [INTEGRATION.md](./INTEGRATION.md) for `external_imports` and event subscriptions
7. **API customisation**: [OPENAPI.md](./OPENAPI.md) for `*.openapi.yaml` overrides
8. **Conventions**: [SCHEMA-STANDARDS.md](./SCHEMA-STANDARDS.md) for naming and required fields
9. **Troubleshooting**: [ERROR_CODES.md](./ERROR_CODES.md) for validation errors
