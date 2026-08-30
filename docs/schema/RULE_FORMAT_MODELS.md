# Model Schema YAML Rules & Format

> **CRITICAL**: Follow these rules exactly to prevent validation errors and code generation failures.

This document is the **canonical authoring reference** for `*.model.yaml` files. It is the merger of the previous DATA_MODEL.md and the field-validator portion of VALIDATION_RULES.md. Every rule here is verified against the YAML parser at [crates/backbone-schema/src/parser/yaml_parser/types.rs](../../crates/backbone-schema/src/parser/yaml_parser/types.rs).

## Table of Contents

1. [File Organization](#file-organization)
2. [Index Model File](#index-model-file)
3. [Entity Model Files](#entity-model-files)
4. [Field Definitions](#field-definitions)
5. [Field Types](#field-types)
6. [Field Attributes](#field-attributes)
7. [Relations](#relations)
8. [Indexes](#indexes)
9. [Enums](#enums)
10. [Shared Types](#shared-types)
11. [Value Objects](#value-objects)
12. [Domain Entities](#domain-entities)
13. [Soft Delete & Audit Metadata](#soft-delete--audit-metadata)
14. [Per-Model Generator Filtering](#per-model-generator-filtering)
15. [Common Mistakes](#common-mistakes)

---

## File Organization

```
libs/modules/{module}/schema/models/
├── index.model.yaml       # Required: Module config, shared types, imports
├── {entity}.model.yaml    # Entity definitions (one or more)
└── ...
```

### Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| File names | `snake_case.model.yaml` | `stored_file.model.yaml` |
| Model names | `PascalCase` | `StoredFile`, `UserQuota` |
| Field names | `snake_case` | `created_at`, `owner_id` |
| Collection names | `snake_case` plural | `stored_files`, `user_quotas` |
| Enum names | `PascalCase` | `FileStatus`, `BucketType` |
| Enum variants | `snake_case` | `pending`, `in_progress` |

---

## Index Model File

The `index.model.yaml` is required and contains module-level configuration.

### Complete Structure

```yaml
# Module metadata (required)
module: module_name
version: 1
description: "Module description"

# Module configuration
config:
  database: postgresql          # postgresql | mongodb
  soft_delete: true             # Enable soft delete (auto-injects metadata column)
  audit: true                   # Enable audit fields
  default_timestamps: true      # Add created_at/updated_at

  # Generator filtering — applies to ALL models in this module
  # See "Per-Model Generator Filtering" section below for details
  generators:
    # Whitelist mode (only these targets generated):
    # enabled: [sql, rust, repository, handler, service, module, config]
    # OR blacklist mode (skip these):
    disabled: [graphql, grpc, proto, openapi]
    # Opt-in CQRS/Projection (off by default):
    cqrs: false

# Shared types (reusable across all models)
shared_types:
  TypeName:
    field_name:
      type: field_type
      attributes: ["@attr"]

  # Composition syntax
  ComposedType: [Type1, Type2]

# Import entity model files
imports:
  - entity_one.model.yaml
  - entity_two.model.yaml

# Optional: Import from other modules
external_imports:
  - module: other_module
    types: [Entity1, Entity2]

# Optional: Value objects with validation
value_objects:
  ValueName:
    inner_type: String
    validation:
      min_length: 1
      max_length: 100
      pattern: "^[a-z]+$"
    methods:
      - name: method_name
        returns: bool

# Optional: Domain services
domain_services:
  ServiceName:
    description: "Service description"
    stateless: true
    dependencies:
      - repo: RepositoryType
    methods:
      - name: method_name
        async: true
        params:
          param1: Type
        returns: "Result<Type, Error>"

# Optional: Use cases
usecases:
  UseCaseName:
    description: "Use case description"
    actor: User
    input:
      field1: Type
    output: ResultType
    steps:
      - step_name
    async: true

# Optional: Domain events
events:
  EventName:
    description: "Event description"
    aggregate: EntityName
    version: 1
    storage:
      store: true
      retention: 2_years
      index_fields: [field1, field2]
    fields:
      - name: field_name
        type: Type

# Optional: Authorization
authorization:
  permissions:
    resource_name: [read, create, update, delete, list]

  roles:
    role_name:
      permissions: ["resource.*"]
      level: 100

  resource_policies:
    EntityName:
      action:
        - policy: policy_name

  policies:
    policy_name:
      description: "Policy description"
      type: any           # any | all
      rules:
        - owner:
            resource: "$this"
            field: owner_id
            actor_field: id
```

---

## Entity Model Files

### Complete Structure

```yaml
# Entity definitions
models:
  - name: EntityName              # Required: PascalCase
    collection: table_name        # Optional: snake_case (auto-inferred if omitted)
    extends: [SharedType1]        # Optional: Inherit fields from shared types
    description: "Description"    # Optional: Entity description

    fields:
      # Field definitions
      field_name:
        type: field_type
        attributes: ["@attr1", "@attr2(value)"]
        description: "Optional description"

      # Shorthand for simple fields
      simple_field: type?         # ? means optional

    relations:
      relation_name:
        type: RelatedEntity[]?    # [] = array, ? = optional
        attributes: ["@one_to_many", "@cascade"]
        description: "Optional description"

    indexes:
      - type: index               # index | unique | fulltext | gin
        fields: [field1, field2]
        name: custom_index_name   # Optional
        where: "condition"        # Optional: partial index

# Enum definitions
enums:
  - name: EnumName
    description: "Enum description"
    variants:
      - name: variant_name
        description: "Variant description"
        default: true             # Optional: mark as default
        value: "CUSTOM_VALUE"     # Optional: custom string value

# Optional: DDD Entity with methods
entities:
  EntityName:
    model: EntityName
    description: "Entity description"
    methods:
      - name: method_name
        params:
          param1: Type
        returns: ReturnType
        mutates: true             # If method modifies entity
    invariants:
      - "field >= 0"
      - "name is unique"
```

---

## Field Definitions

### Full Syntax

```yaml
fields:
  field_name:
    type: field_type              # Required
    attributes: ["@attr1"]        # Optional
    description: "Description"    # Optional
```

### Shorthand Syntax

```yaml
fields:
  # Required field
  name: string

  # Optional field (nullable)
  description: string?

  # Array field
  tags: "string[]"

  # Optional array field
  metadata: "json?"
```

### Special Fields

```yaml
fields:
  # Primary key (UUID)
  id:
    type: uuid
    attributes: ["@id", "@default(uuid)"]

  # Timestamps (usually from shared type Metadata)
  created_at:
    type: datetime
    attributes: ["@default(now)"]
  updated_at:
    type: datetime
    attributes: ["@updated_at"]
  deleted_at: datetime?           # For soft delete

  # Foreign key
  owner_id:
    type: uuid
    attributes: ["@required"]

  # Status enum with default
  status:
    type: StatusEnum
    attributes: ["@default(pending)"]
```

---

## Field Types

### Primitive Types

| Type | Description | PostgreSQL | Rust |
|------|-------------|------------|------|
| `string` | Text | `VARCHAR` / `TEXT` | `String` |
| `int` | 32-bit integer | `INTEGER` | `i32` |
| `bigint` | 64-bit integer (alias: `int64`) | `BIGINT` | `i64` |
| `float` | 64-bit float | `DOUBLE PRECISION` | `f64` |
| `decimal` | Decimal | `DECIMAL(p,s)` | `Decimal` |
| `bool` | Boolean | `BOOLEAN` | `bool` |
| `uuid` | UUID | `UUID` | `Uuid` |
| `datetime` | Timestamp | `TIMESTAMPTZ` | `DateTime<Utc>` |
| `date` | Date only | `DATE` | `NaiveDate` |
| `time` | Time only | `TIME` | `NaiveTime` |
| `json` | JSON data | `JSONB` | `serde_json::Value` |
| `bytes` | Binary | `BYTEA` | `Vec<u8>` |

### String Format Types

| Type | Description | Validates |
|------|-------------|-----------|
| `email` | Email address | RFC 5322 |
| `url` | URL | Valid URL format |
| `phone` | Phone number | E.164 format |
| `ip` | IP address | IPv4 or IPv6 |
| `ipv4` | IPv4 address | IPv4 only |
| `ipv6` | IPv6 address | IPv6 only |

### Type Modifiers

```yaml
# Optional (nullable)
field: type?

# Array
field: "type[]"

# Optional array
field: "type[]?"

# Cross-module reference
field: module_name.EntityName
```

---

## Field Attributes

### Identity & Keys

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@id` | Primary key | `["@id"]` |
| `@unique` | Unique constraint | `["@unique"]` |
| `@foreign_key(field)` | Foreign key reference | `["@foreign_key(user_id)"]` |

### Required & Defaults

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@required` | NOT NULL | `["@required"]` |
| `@default(value)` | Default value | `["@default(0)"]` |
| `@default(uuid)` | Auto UUID | `["@default(uuid)"]` |
| `@default(now)` | Current timestamp | `["@default(now)"]` |
| `@default(cuid)` | CUID | `["@default(cuid)"]` |
| `@default(ulid)` | ULID | `["@default(ulid)"]` |
| `@updated_at` | Auto-update timestamp | `["@updated_at"]` |

### String Validation

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@min(n)` | Min length | `["@min(1)"]` |
| `@max(n)` | Max length | `["@max(255)"]` |
| `@length(n)` | Exact length | `["@length(32)"]` |
| `@pattern(regex)` | Regex pattern | `["@pattern('^[a-z]+$')"]` |
| `@email` | Email format | `["@email"]` |
| `@url` | URL format | `["@url"]` |
| `@slug` | URL-safe slug | `["@slug"]` |
| `@alpha` | Letters only | `["@alpha"]` |
| `@alpha_num` | Alphanumeric | `["@alpha_num"]` |
| `@alpha_dash` | Alphanumeric + dash/underscore | `["@alpha_dash"]` |
| `@lowercase` | Must be lowercase | `["@lowercase"]` |
| `@uppercase` | Must be uppercase | `["@uppercase"]` |
| `@starts_with(val)` | Must start with | `["@starts_with('http')"]` |
| `@ends_with(val)` | Must end with | `["@ends_with('.pdf')"]` |
| `@contains(val)` | Must contain | `["@contains('@')"]` |

### Numeric Validation

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@positive` | > 0 | `["@positive"]` |
| `@negative` | < 0 | `["@negative"]` |
| `@non_negative` | >= 0 | `["@non_negative"]` |
| `@range(min,max)` | Value range | `["@range(1,100)"]` |
| `@multiple_of(n)` | Divisible by | `["@multiple_of(5)"]` |
| `@precision(p,s)` | Decimal precision | `["@precision(10,2)"]` |

### Date/Time Validation

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@past` | Must be in past | `["@past"]` |
| `@future` | Must be in future | `["@future"]` |
| `@after(date)` | After date | `["@after('2024-01-01')"]` |
| `@before(date)` | Before date | `["@before('2030-01-01')"]` |
| `@timezone` | Valid timezone | `["@timezone"]` |

### Array Validation

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@min_items(n)` | Min array length | `["@min_items(1)"]` |
| `@max_items(n)` | Max array length | `["@max_items(10)"]` |
| `@distinct` | No duplicates | `["@distinct"]` |

### Choice Validation

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@in(vals)` | Must be one of | `["@in('a','b','c')"]` |
| `@not_in(vals)` | Must not be one of | `["@not_in('x','y')"]` |
| `@enum(Type)` | Enum type | `["@enum(Status)"]` |

### Cross-Field Validation

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@same(field)` | Must match field | `["@same(password)"]` |
| `@different(field)` | Must differ from field | `["@different(old_password)"]` |
| `@gt(field)` | Greater than field | `["@gt(start_date)"]` |
| `@gte(field)` | Greater or equal to field | `["@gte(min_value)"]` |
| `@lt(field)` | Less than field | `["@lt(max_value)"]` |
| `@lte(field)` | Less or equal to field | `["@lte(end_date)"]` |
| `@required_with(fields)` | Required if fields present | `["@required_with(field1,field2)"]` |
| `@required_if(field,val)` | Required if field equals | `["@required_if(type,'custom')"]` |

### Security & Database

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@sensitive` | Mark as sensitive | `["@sensitive"]` |
| `@encrypted` | Encrypt at rest | `["@encrypted"]` |
| `@hashed` | Hash value | `["@hashed"]` |
| `@unique_where(cond)` | Conditional unique | `["@unique_where(deleted_at IS NULL)"]` |
| `@exists(table,col)` | FK exists check | `["@exists(users,id)"]` |

---

## Relations

### Relation Types

```yaml
relations:
  # One-to-One
  profile:
    type: Profile
    attributes: ["@one"]

  # One-to-Many
  files:
    type: StoredFile[]
    attributes: ["@one_to_many"]

  # Many-to-Many
  roles:
    type: Role[]
    attributes: ["@many_to_many"]

  # Optional relation
  manager:
    type: User?
    attributes: ["@one"]
```

### Relation Attributes

| Attribute | Description | Example |
|-----------|-------------|---------|
| `@one` | One-to-one | `["@one"]` |
| `@one_to_many` | One-to-many | `["@one_to_many"]` |
| `@many_to_many` | Many-to-many | `["@many_to_many"]` |
| `@foreign_key(field)` | Explicit FK field | `["@foreign_key(manager_id)"]` |
| `@join_table(name)` | Explicit join table | `["@join_table(user_roles)"]` |
| `@cascade` | Cascade delete | `["@cascade"]` |
| `@set_null` | Set null on delete | `["@set_null"]` |
| `@restrict` | Restrict delete | `["@restrict"]` |

### Self-Referencing Relations

```yaml
# IMPORTANT: Self-references ALWAYS require @foreign_key
relations:
  parent:
    type: Category?
    attributes: ["@one", "@foreign_key(parent_id)"]

  children:
    type: Category[]
    attributes: ["@one_to_many", "@foreign_key(parent_id)"]

  manager:
    type: User?
    attributes: ["@one", "@foreign_key(manager_id)"]
```

---

## Indexes

### Index Types

```yaml
indexes:
  # Regular index (for queries)
  - type: index
    fields: [field1, field2]

  # Unique constraint
  - type: unique
    fields: [email]

  # Full-text search
  - type: fulltext
    fields: [title, content]

  # GIN index (for JSONB/arrays)
  - type: gin
    fields: [metadata]
```

### Index Options

```yaml
indexes:
  # Named index
  - type: index
    name: idx_user_status_created
    fields: [status, created_at]

  # Partial index (PostgreSQL)
  - type: unique
    fields: [email]
    where: "deleted_at IS NULL"

  # Composite unique
  - type: unique
    fields: [org_id, slug]
```

---

## Enums

### Full Enum Definition

```yaml
enums:
  - name: Status
    description: "Entity status"
    variants:
      - name: pending
        description: "Awaiting processing"
        default: true
      - name: active
        description: "Currently active"
      - name: suspended
        description: "Temporarily suspended"
      - name: deleted
        description: "Soft deleted"

  # Enum with custom values
  - name: Priority
    variants:
      - name: low
        value: "LOW"
      - name: medium
        value: "MEDIUM"
        default: true
      - name: high
        value: "HIGH"
      - name: critical
        value: "CRITICAL"
```

### Rules for Enums

1. **Name**: Must be `PascalCase`
2. **Variants**: Must be `snake_case`
3. **Default**: Only ONE variant can have `default: true`
4. **Values**: If using custom values, be consistent

---

## Shared Types

### Definition in index.model.yaml

```yaml
shared_types:
  # Simple shared type
  Timestamps:
    created_at:
      type: datetime
      attributes: ["@default(now)"]
    updated_at:
      type: datetime
      attributes: ["@updated_at"]
    deleted_at: datetime?

  # Another shared type
  Actors:
    created_by: uuid?
    updated_by: uuid?

  # Composed type (combines other types)
  Metadata: [Timestamps, Actors]
```

### Using Shared Types

```yaml
# In entity model - as column inheritance
models:
  - name: User
    extends: [Metadata]           # Fields become columns
    fields:
      name: string

# In entity model - as JSONB field
models:
  - name: Order
    fields:
      metadata: Metadata          # Stored as JSONB
```

---

## Value Objects

### Definition

```yaml
value_objects:
  Email:
    inner_type: String
    validation:
      pattern: "^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$"
    methods:
      - name: domain
        returns: "&str"
        description: "Get email domain"

  Money:
    fields:
      amount:
        type: Decimal
        validation:
          min: 0
      currency:
        type: String
        validation:
          length: 3
    methods:
      - name: add
        params:
          other: Money
        returns: "Result<Money, CurrencyMismatch>"
```

---

## Domain Entities

### Enhanced Entity Definition

```yaml
entities:
  Order:
    model: Order
    description: "Order aggregate root"

    methods:
      - name: add_item
        params:
          product_id: Uuid
          quantity: u32
          price: Money
        returns: "Result<(), OrderError>"
        mutates: true
        description: "Add item to order"

      - name: calculate_total
        returns: Money
        description: "Calculate order total"

      - name: can_cancel
        returns: bool
        description: "Check if order can be cancelled"

    invariants:
      - "items.len() > 0 when status != draft"
      - "total >= 0"
      - "customer_id is set when status != draft"
```

---

## Soft Delete & Audit Metadata

### Soft Delete

When `soft_delete: true` is set on a model (or globally in `index.model.yaml` config), the SQL generator **automatically injects a `metadata` JSONB column** containing `deleted_at`, `deleted_by`, and timestamp fields. You do NOT need to declare these as separate columns.

```yaml
models:
  - name: Order
    collection: orders
    soft_delete: true             # Auto-injects metadata JSONB column
    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]
      # ... your fields
      # NOTE: do NOT declare deleted_at/deleted_by manually when soft_delete: true
```

The repository layer applies a `WHERE metadata->>'deleted_at' IS NULL` filter automatically on read queries, and `delete()` performs a soft update of the metadata field rather than a hard `DELETE`.

### Audit Metadata Pattern

The recommended pattern is to declare a `metadata` field of the shared `Metadata` type with the `@audit_metadata` attribute. This stores audit fields as a single JSONB column rather than separate columns:

```yaml
fields:
  metadata:
    type: Metadata
    attributes: ["@audit_metadata"]
    description: "Audit metadata (created_at, updated_at, deleted_at, created_by, updated_by, deleted_by)"
```

The `Metadata` shared type is defined in `index.model.yaml` and composes `Timestamps` + `Actors`. See [TYPES.md](./TYPES.md) for the full composition rules. Fields stored under `@audit_metadata` are automatically excluded from generated Create/Update DTOs.

---

## Per-Model Generator Filtering

Each model can override which generators run for it. This is useful when an entity should not have GraphQL exposure, gRPC services, OpenAPI specs, or CQRS projections.

### Module-Level Filtering

In `index.model.yaml`:

```yaml
config:
  generators:
    # Whitelist mode — only these targets run for the entire module:
    enabled: [sql, rust, repository, handler, service, module, config]

    # OR blacklist mode — these targets are skipped for the entire module:
    disabled: [graphql, grpc, proto, openapi]

    # Opt-in CQRS/Projection (off by default — must be set to true to enable):
    cqrs: false
```

### Per-Entity Override

In an entity model file:

```yaml
models:
  - name: AuditLog
    collection: audit_logs
    fields:
      # ...
    generators:
      disabled: [handler, grpc, graphql, openapi, dto]   # Internal entity, no public API
```

Per-model `generators:` takes precedence over module-level config. The whitelist (`enabled`) and blacklist (`disabled`) modes are mutually exclusive — if `enabled` is set, only listed targets run; otherwise `disabled` removes the listed targets.

### Available Targets

See [GENERATION.md](./GENERATION.md) for the full list of 37 generation targets and their target names (`sql`, `rust`, `repository`, `handler`, etc.).

---

## Common Mistakes

### 1. Missing @id on Primary Key

```yaml
# WRONG
fields:
  id:
    type: uuid
    attributes: ["@default(uuid)"]

# CORRECT
fields:
  id:
    type: uuid
    attributes: ["@id", "@default(uuid)"]
```

### 2. Self-Reference Without @foreign_key

```yaml
# WRONG - Will fail
relations:
  parent:
    type: Category?

# CORRECT
relations:
  parent:
    type: Category?
    attributes: ["@one", "@foreign_key(parent_id)"]
```

### 3. Using @required on Optional Fields

```yaml
# WRONG - Contradictory
fields:
  name:
    type: string?                 # ? means optional
    attributes: ["@required"]     # But required?!

# CORRECT
fields:
  name:
    type: string
    attributes: ["@required"]
```

### 4. Wrong Enum Default Syntax

```yaml
# WRONG - In field
fields:
  status:
    type: Status
    default: active              # Wrong syntax

# CORRECT - Using attribute
fields:
  status:
    type: Status
    attributes: ["@default(active)"]
```

### 5. Missing Quotes for Array Types

```yaml
# WRONG - YAML parsing issue
fields:
  tags: string[]

# CORRECT - Quote the type
fields:
  tags: "string[]"
```

### 6. Wrong Collection Name Format

```yaml
# WRONG
models:
  - name: UserProfile
    collection: UserProfiles     # PascalCase

# CORRECT
models:
  - name: UserProfile
    collection: user_profiles    # snake_case
```

### 7. Duplicate Index Names

```yaml
# WRONG - Implicit duplicate names
indexes:
  - type: index
    fields: [user_id]
  - type: index
    fields: [user_id, status]    # Same prefix

# CORRECT - Explicit unique names
indexes:
  - type: index
    name: idx_user_id
    fields: [user_id]
  - type: index
    name: idx_user_status
    fields: [user_id, status]
```

### 8. External Reference Without Import

```yaml
# WRONG - Missing import
fields:
  user:
    type: sapiens.User           # Not imported

# CORRECT - With import in index.model.yaml
external_imports:
  - module: sapiens
    types: [User]

# Then in entity file
fields:
  user:
    type: sapiens.User
```

---

## Quick Reference Checklist

- [ ] `index.model.yaml` has `module`, `version`, `imports`
- [ ] Model names are `PascalCase`
- [ ] Field names are `snake_case`
- [ ] Collection names are `snake_case` plural
- [ ] Primary key has `@id` and `@default(uuid)`
- [ ] Foreign keys have `@required` (unless optional)
- [ ] Self-references have `@foreign_key(field)`
- [ ] Optional fields use `?` suffix
- [ ] Array types are quoted: `"string[]"`
- [ ] Enums have exactly one `default: true` variant
- [ ] Indexes have unique names (explicit or implicit)
- [ ] External types are imported in `external_imports`
