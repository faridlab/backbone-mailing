# Type System Reference

Complete reference for all types supported in the Backbone Schema System.

> **Note**: This is a deep reference. For day-to-day field declaration, see [RULE_FORMAT_MODELS.md](./RULE_FORMAT_MODELS.md). The Field Types and Field Attributes tables there cover the 90% case. Read this document when you need composition rules, value-object construction, or proto / SQL mappings.

## Table of Contents

- [Primitive Types](#primitive-types)
- [Special Types](#special-types)
- [Collection Types](#collection-types)
- [Custom Types](#custom-types)
- [Shared Type Composition](#shared-type-composition)
- [Value Objects & Typed IDs](#value-objects--typed-ids)
- [Type Naming Conventions](#type-naming-conventions)
- [Type Mappings](#type-mappings)
- [Nullability](#nullability)
- [Type Coercion](#type-coercion)
- [Validation Attributes Quick Reference](#validation-attributes-quick-reference)

---

## Primitive Types

### String Types

| Type | Description | Rust | PostgreSQL | Proto |
|------|-------------|------|------------|-------|
| `string` | Variable length text | `String` | `TEXT` | `string` |
| `string(n)` | Fixed max length | `String` | `VARCHAR(n)` | `string` |
| `text` | Long text | `String` | `TEXT` | `string` |
| `char(n)` | Fixed length | `String` | `CHAR(n)` | `string` |

```yaml
fields:
  name:
    type: string                 # TEXT

  code:
    type: string
    attributes: ["@max(10)"]     # VARCHAR(10)

  description:
    type: text                   # TEXT

  country:
    type: string
    attributes: ["@length(2)"]   # CHAR(2)
```

### Numeric Types

| Type | Description | Rust | PostgreSQL | Proto |
|------|-------------|------|------------|-------|
| `int` | 32-bit integer | `i32` | `INTEGER` | `int32` |
| `int8` | 8-bit integer | `i8` | `SMALLINT` | `int32` |
| `int16` | 16-bit integer | `i16` | `SMALLINT` | `int32` |
| `int32` | 32-bit integer | `i32` | `INTEGER` | `int32` |
| `int64` | 64-bit integer | `i64` | `BIGINT` | `int64` |
| `uint` | Unsigned 32-bit | `u32` | `INTEGER` | `uint32` |
| `uint64` | Unsigned 64-bit | `u64` | `BIGINT` | `uint64` |
| `float` | 32-bit float | `f32` | `REAL` | `float` |
| `float64` | 64-bit float | `f64` | `DOUBLE PRECISION` | `double` |
| `decimal` | Exact decimal | `Decimal` | `NUMERIC` | `string` |

```yaml
fields:
  count:
    type: int                    # INTEGER

  big_count:
    type: int64                  # BIGINT

  price:
    type: decimal                # NUMERIC

  amount:
    type: float64                # DOUBLE PRECISION

  percentage:
    type: float                  # REAL
```

### Decimal Precision

```yaml
fields:
  price:
    type: decimal
    attributes: ["@precision(10, 2)"]    # NUMERIC(10, 2)

  rate:
    type: decimal
    attributes: ["@precision(5, 4)"]     # NUMERIC(5, 4)

  amount:
    type: decimal
    attributes: ["@precision(19, 4)"]    # NUMERIC(19, 4) - money
```

### Boolean

| Type | Description | Rust | PostgreSQL | Proto |
|------|-------------|------|------------|-------|
| `bool` | True/False | `bool` | `BOOLEAN` | `bool` |

```yaml
fields:
  is_active:
    type: bool                   # BOOLEAN

  verified:
    type: bool
    attributes: ["@default(false)"]
```

### Date and Time

| Type | Description | Rust | PostgreSQL | Proto |
|------|-------------|------|------------|-------|
| `datetime` | Date and time with TZ | `DateTime<Utc>` | `TIMESTAMPTZ` | `google.protobuf.Timestamp` |
| `date` | Date only | `NaiveDate` | `DATE` | `string` |
| `time` | Time only | `NaiveTime` | `TIME` | `string` |
| `timestamp` | Unix timestamp | `i64` | `BIGINT` | `int64` |
| `duration` | Time duration | `Duration` | `INTERVAL` | `google.protobuf.Duration` |

```yaml
fields:
  created_at:
    type: datetime               # TIMESTAMPTZ

  birth_date:
    type: date                   # DATE

  start_time:
    type: time                   # TIME

  expires_at:
    type: timestamp              # BIGINT (unix)

  ttl:
    type: duration               # INTERVAL
```

### Identifiers

| Type | Description | Rust | PostgreSQL | Proto |
|------|-------------|------|------------|-------|
| `uuid` | UUID v4 | `Uuid` | `UUID` | `string` |
| `cuid` | CUID | `String` | `VARCHAR(25)` | `string` |
| `ulid` | ULID | `String` | `VARCHAR(26)` | `string` |

```yaml
fields:
  id:
    type: uuid
    attributes: ["@id", "@default(uuid)"]

  public_id:
    type: cuid
    attributes: ["@default(cuid)"]

  sort_id:
    type: ulid
    attributes: ["@default(ulid)"]
```

---

## Special Types

### Validated String Types

These types are shortcuts that combine a base type with built-in validation:

| Type | Validation | Example | Equivalent |
|------|------------|---------|------------|
| `email` | Email format | `user@example.com` | `string @email` |
| `url` | URL format | `https://example.com` | `string @url` |
| `phone` | Phone format | `+1-555-555-5555` | `string @phone` |
| `ip` | IP address | `192.168.1.1` | `string @ip` |
| `ipv4` | IPv4 address | `192.168.1.1` | `string @ipv4` |
| `ipv6` | IPv6 address | `::1` | `string @ipv6` |
| `slug` | URL-safe string | `my-blog-post` | `string @slug` |
| `username` | Alphanumeric + underscore | `john_doe` | `string @alpha_dash` |
| `domain` | Domain name | `example.com` | `string @domain` |
| `hostname` | Hostname | `server-01.local` | `string @hostname` |
| `mac_address` | MAC address | `00:1B:44:11:3A:B7` | `string @mac_address` |
| `hex_color` | Hex color | `#ff5733` | `string @hex_color` |
| `credit_card` | Credit card number | `4111111111111111` | `string @credit_card` |
| `iban` | IBAN number | `DE89370400440532013000` | `string @iban` |
| `isbn` | ISBN-10 or ISBN-13 | `978-3-16-148410-0` | `string @isbn` |
| `country_code` | ISO 3166-1 alpha-2 | `US` | `string @country_code` |
| `currency_code` | ISO 4217 | `USD` | `string @currency_code` |
| `language_code` | ISO 639-1 | `en` | `string @language_code` |
| `locale` | Locale code | `en_US` | `string @locale` |
| `timezone` | Timezone | `America/New_York` | `string @timezone` |

```yaml
fields:
  email:
    type: email
    attributes: ["@required"]     # Validates email format

  website:
    type: url?                    # Validates URL format

  phone:
    type: phone?                  # Validates phone format

  ip_address:
    type: ip?                     # Validates IP format

  country:
    type: country_code            # ISO country code

  currency:
    type: currency_code           # ISO currency code
```

These are shortcuts for:

```yaml
fields:
  email:
    type: string
    attributes: ["@email", "@required"]

  website:
    type: string?
    attributes: ["@url"]

  country:
    type: string
    attributes: ["@country_code"]
```

### Password Type

Special type for password fields with built-in security:

```yaml
fields:
  password:
    type: password
    attributes: ["@required"]    # Implies @sensitive @hashed
```

Equivalent to:

```yaml
fields:
  password:
    type: string
    attributes: ["@min(8)", "@password", "@sensitive", "@hashed", "@required"]
```

### File Types

Types for file handling (typically used with external storage):

| Type | Description | Validation |
|------|-------------|------------|
| `file` | Generic file reference | Valid file path/URL |
| `image` | Image file reference | Valid image (jpg, png, gif, webp, svg) |
| `document` | Document file reference | Valid document (pdf, doc, docx, etc.) |

```yaml
fields:
  avatar:
    type: image?                 # Image file URL

  resume:
    type: document?              # Document file URL

  attachment:
    type: file?                  # Any file URL
```

With size and dimension constraints:

```yaml
fields:
  avatar:
    type: image
    attributes: ["@max_size(2097152)", "@dimensions(min_width: 100, max_width: 500)"]

  thumbnail:
    type: image
    attributes: ["@aspect_ratio(1:1)", "@max_size(102400)"]

  document:
    type: file
    attributes: ["@extensions(pdf, docx)", "@max_size(10485760)"]
```

### Binary Types

| Type | Description | Rust | PostgreSQL | Proto |
|------|-------------|------|------------|-------|
| `bytes` | Binary data | `Vec<u8>` | `BYTEA` | `bytes` |
| `blob` | Large binary | `Vec<u8>` | `BYTEA` | `bytes` |

```yaml
fields:
  avatar:
    type: bytes?                 # Binary image data

  document:
    type: blob?                  # Large binary file
```

### Structured Types

| Type | Description | Rust | PostgreSQL | Proto |
|------|-------------|------|------------|-------|
| `json` | JSON data | `serde_json::Value` | `JSONB` | `google.protobuf.Struct` |
| `jsonb` | Binary JSON | `serde_json::Value` | `JSONB` | `google.protobuf.Struct` |

```yaml
fields:
  metadata:
    type: json?                  # Arbitrary JSON

  settings:
    type: jsonb?                 # Binary JSON (faster queries)

  config:
    type: json
    attributes: ["@default({})"] # Default empty object
```

---

## Collection Types

### Arrays

Use `[]` suffix for array types:

```yaml
fields:
  tags:
    type: string[]               # Array of strings

  scores:
    type: int[]                  # Array of integers

  coordinates:
    type: float[]                # Array of floats
```

| Schema | Rust | PostgreSQL | Proto |
|--------|------|------------|-------|
| `string[]` | `Vec<String>` | `TEXT[]` | `repeated string` |
| `int[]` | `Vec<i32>` | `INTEGER[]` | `repeated int32` |
| `uuid[]` | `Vec<Uuid>` | `UUID[]` | `repeated string` |

### Maps

```yaml
fields:
  labels:
    type: map<string, string>    # Key-value pairs

  counts:
    type: map<string, int>       # String to int map
```

| Schema | Rust | PostgreSQL | Proto |
|--------|------|------------|-------|
| `map<K, V>` | `HashMap<K, V>` | `JSONB` | `map<K, V>` |

---

## Custom Types

Custom types allow you to define reusable field structures. They can be used in three ways:
1. **Extends** - Inject fields as table columns (type inheritance)
2. **JSONB field** - Store as JSONB column with validation
3. **Type composition** - Combine multiple types into one

### Defining Custom Types

Define reusable composite types in `index.model.yaml`:

```yaml
# In index.model.yaml

shared_types:
  Money:
    amount:
      type: decimal
      attributes: ["@precision(19, 4)"]
    currency:
      type: string
      attributes: ["@default(USD)", "@max(3)"]

  Address:
    street:
      type: string
      attributes: ["@required", "@max(255)"]
    street2:
      type: string?
      attributes: ["@max(255)"]
    city:
      type: string
      attributes: ["@required", "@max(100)"]
    state:
      type: string?
      attributes: ["@max(100)"]
    country:
      type: string
      attributes: ["@required", "@default(ID)", "@max(2)"]
    postal_code:
      type: string?
      attributes: ["@max(20)"]

  Coordinates:
    latitude:
      type: float64
      attributes: ["@range(-90, 90)"]
    longitude:
      type: float64
      attributes: ["@range(-180, 180)"]

  ContactInfo:
    email:
      type: email
      attributes: ["@required"]
    phone:
      type: phone?
    fax:
      type: phone?

  # Base audit types
  Timestamps:
    created_at:
      type: datetime
      attributes: ["@default(now)"]
    updated_at:
      type: datetime
      attributes: ["@updated_at"]
    deleted_at:
      type: datetime?

  Actors:
    created_by:
      type: uuid?
    updated_by:
      type: uuid?
    deleted_by:
      type: uuid?

  # Type composition - combine multiple types
  Metadata: [Timestamps, Actors]
```

### Type Composition

You can compose types from other types using array syntax:

```yaml
shared_types:
  Timestamps:
    created_at:
      type: datetime
      attributes: ["@default(now)"]
    updated_at:
      type: datetime
      attributes: ["@updated_at"]

  Actors:
    created_by: uuid?
    updated_by: uuid?

  # Metadata = Timestamps + Actors (6 fields total)
  Metadata: [Timestamps, Actors]
```

The composed type `Metadata` will contain all fields from both `Timestamps` and `Actors`.

### Using Custom Types

#### 1. Type Inheritance with `extends`

Use `extends` to inject shared type fields as **direct table columns**:

```yaml
models:
  - name: Order
    collection: orders
    extends: [Metadata]  # All Metadata fields become columns

    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]

      total:
        type: decimal
```

**Result:** The `orders` table will have columns: `id`, `total`, `created_at`, `updated_at`, `deleted_at`, `created_by`, `updated_by`, `deleted_by`

**Key behaviors:**
- Extended fields are injected as **direct columns**
- Explicit fields override inherited fields with the same name
- Multiple types can be extended: `extends: [Timestamps, Actors]`

#### 2. Shared Type as JSONB Field

Use a shared type name as the field type to store as **JSONB**:

```yaml
models:
  - name: Order
    collection: orders

    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]

      shipping:
        type: Address              # Stored as JSONB column
        attributes: ["@required"]

      billing:
        type: Address?             # Optional JSONB column
```

**Generated SQL:**
```sql
CREATE TABLE orders (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    shipping JSONB NOT NULL CHECK (
        shipping ? 'street' AND
        shipping ? 'city' AND
        shipping ? 'country'
    ),
    billing JSONB
);
```

**Key behaviors:**
- The column type is `JSONB`
- PostgreSQL CHECK constraints enforce required fields
- Non-required fields in the type don't generate CHECK constraints
- The `@jsonb_type` and `@jsonb_schema` attributes are added internally

### File-Level Types

Define types at the file level (same level as `models:` and `enums:`) for reuse across multiple models:

```yaml
# File-level types - reusable across all models in this file
types:
  - name: Address
    fields:
      street:
        type: string
        attributes: ["@required"]
      city:
        type: string
        attributes: ["@required"]
      zip:
        type: string?

  - name: LineItem
    fields:
      product_id:
        type: uuid
        attributes: ["@required"]
      quantity:
        type: int
        attributes: ["@required", "@positive"]
      unit_price:
        type: decimal
        attributes: ["@precision(10, 2)"]

models:
  - name: Customer
    collection: customers
    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]
      billing_address: Address     # Reuse file-level type
      shipping_address: Address    # Same type, different field

  - name: Order
    collection: orders
    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]
      items:
        type: LineItem[]           # Reuse as array
      delivery: Address            # Same Address type
```

**Key behaviors:**
- File-level types are defined alongside `models:` and `enums:`
- They can be reused across **all models** in the same file
- They work the same as shared types (stored as JSONB)
- File-level types take precedence over shared types with the same name
- More flexible than model-local types for multi-model files

### Type with Validation

```yaml
shared_types:
  Password:
    value:
      type: string
      attributes: ["@min(8)", "@max(128)", "@sensitive", "@hashed"]

  Percentage:
    value:
      type: float
      attributes: ["@range(0, 100)"]

  PositiveAmount:
    value:
      type: decimal
      attributes: ["@precision(19, 4)", "@min(0)"]
```

### JSONB Validation

When using shared types as JSONB fields, validation is enforced at the database level:

| Attribute | PostgreSQL CHECK |
|-----------|------------------|
| `@required` | `column ? 'field_name'` |
| Non-required (optional) | No CHECK constraint |

**Example:**
```yaml
shared_types:
  Settings:
    theme:
      type: string
      attributes: ["@required"]     # Will have CHECK
    notifications:
      type: bool                    # No CHECK (optional)
    language:
      type: string
      attributes: ["@default(en)"]  # No CHECK (has default)
```

**Generated CHECK:**
```sql
CHECK (settings ? 'theme')
```

---

## Shared Type Composition

Shared types are reusable field bundles defined once in `index.model.yaml` and reused across many models. There are two ways to bring them into a model:

1. **`extends:` (column injection)** — fields become individual table columns.
2. **JSONB field with `@audit_metadata`** — fields are stored as a single JSONB column.

### Defining Shared Types

```yaml
# index.model.yaml
shared_types:
  Timestamps:
    created_at:
      type: datetime
      attributes: ["@default(now)"]
    updated_at:
      type: datetime
      attributes: ["@updated_at"]
    deleted_at: datetime?

  Actors:
    created_by:
      type: uuid?
      attributes: ["@foreign_key(User.id)"]
    updated_by:
      type: uuid?
      attributes: ["@foreign_key(User.id)"]
    deleted_by:
      type: uuid?
      attributes: ["@foreign_key(User.id)"]

  # Composition: combine other shared types
  Metadata: [Timestamps, Actors]

  # Domain-specific bundles
  Money:
    amount:
      type: decimal
      attributes: ["@precision(18,2)", "@non_negative"]
    currency:
      type: string
      attributes: ["@default('IDR')", "@length(3)"]

  GeoLocation:
    latitude:
      type: float?
      attributes: ["@range(-90, 90)"]
    longitude:
      type: float?
      attributes: ["@range(-180, 180)"]
```

### Using Shared Types as Columns (`extends`)

```yaml
models:
  - name: Order
    extends: [Metadata]   # injects created_at, updated_at, deleted_at, created_by, updated_by, deleted_by as columns
    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]
      total: decimal
```

The generated SQL has six extra columns. Use this when you want to query or index audit fields directly (`WHERE created_at > ...`).

### Using Shared Types as JSONB Fields

```yaml
models:
  - name: Order
    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]
      total: decimal
      metadata:
        type: Metadata
        attributes: ["@audit_metadata"]
```

This produces a single `metadata JSONB` column. The `@audit_metadata` attribute also tells DTOs to **exclude** this field from Create/Update request bodies.

> **Special case**: The shared type literally named `Metadata` is treated as the audit-metadata type. The constant lives at `crates/backbone-schema/src/parser/yaml_parser/types.rs:14` (`AUDIT_METADATA_TYPE_NAME`).

### When to Use Which

| Use case | Pattern |
|----------|---------|
| You query or index audit fields | `extends: [Metadata]` |
| You want a clean schema with audit data tucked away | `metadata: Metadata` with `@audit_metadata` |
| You want to share validation rules and field shapes | Either; pick by query needs |
| You want fields excluded from Create/Update DTOs | `@audit_metadata` only |

---

## Value Objects & Typed IDs

The `value-object` generator turns plain types into newtype-wrapped Rust types with validation, conversion, and equality semantics.

### Typed IDs

Every entity gets a typed ID by default — `OrderId`, `UserId`, etc. — backed by `Uuid` but distinct at the type level. The generator emits:

- `From<Uuid>` and `Into<Uuid>`
- `From<&Uuid>` and `From<&str>` and `From<&String>` (recent additions)
- `AsRef<Uuid>` and `Deref<Target = Uuid>` so typed IDs work with anything that takes `&Uuid`
- `Display`, `Debug`, `serde::Serialize`, `serde::Deserialize`

This means you can pass `&order_id` to a function expecting `&Uuid`, and the type system still distinguishes `OrderId` from `UserId` so you can't accidentally swap them.

### Wrapper Value Objects

```yaml
# index.model.yaml
value_objects:
  Email:
    inner_type: String
    validation:
      pattern: "^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$"
    methods:
      - name: domain
        returns: "&str"

  PhoneNumber:
    inner_type: String
    validation:
      pattern: "^\\+?[1-9]\\d{1,14}$"
```

Generated:

```rust
pub struct Email(String);

impl Email {
    pub fn try_new(value: impl Into<String>) -> Result<Self, ValidationError> { ... }
    pub fn domain(&self) -> &str { ... }
}

impl From<&str> for Email { /* with validation */ }
impl AsRef<str> for Email { ... }
```

### Composite Value Objects

```yaml
value_objects:
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

Use composite VOs when two or more fields must travel together as a single concept (money, geo coordinates, address pairs).

---

## Type Naming Conventions

### Automatic PascalCase Conversion

When you define custom enum types in your schema, the code generator automatically converts them to **PascalCase** to follow Rust naming conventions.

**Schema Definition:**
```yaml
# In your model file
enums:
  - name: relationship_type
    values: [lead, customer, partner]

  - name: contact_type
    values: [email, phone, mail]

  - name: opportunity_status
    values: [open, won, lost]

models:
  - name: Contact
    fields:
      relationship:
        type: relationship_type    # lowercase in schema
      contact_method:
        type: contact_type         # lowercase in schema
      status:
        type: opportunity_status   # lowercase in schema
```

**Generated Rust Code:**
```rust
// Enum definitions - automatically converted to PascalCase
pub enum RelationshipType {
    Lead,
    Customer,
    Partner,
}

pub enum ContactType {
    Email,
    Phone,
    Mail,
}

pub enum OpportunityStatus {
    Open,
    Won,
    Lost,
}

// Entity struct uses PascalCase enum types
pub struct Contact {
    pub relationship: RelationshipType,    // PascalCase
    pub contact_method: ContactType,       // PascalCase
    pub status: OpportunityStatus,         // PascalCase
}
```

**Key Benefits:**
- ✅ Follows Rust naming conventions (PascalCase for types)
- ✅ Prevents compilation errors from lowercase type names
- ✅ Maintains consistency across generated code
- ✅ You can use lowercase in schema (more readable)
- ✅ Generator handles conversion automatically

**Affected Generators:**
- **Entity generator** - PascalCase enum definitions
- **Validator generator** - PascalCase type references
- **Handler generator** - PascalCase in request/response types
- **DTO generator** - PascalCase in data transfer objects
- **gRPC generator** - PascalCase in proto definitions

**Module Visibility:**
All entity and enum modules are automatically declared as `pub mod` to allow external access from other modules:

```rust
// In entity/mod.rs
pub mod relationship_type;    // Public module
pub mod contact_type;         // Public module
pub mod opportunity_status;   // Public module
```

---

## Type Mappings

### Schema to Rust

| Schema Type | Rust Type |
|-------------|-----------|
| `string` | `String` |
| `int` | `i32` |
| `int64` | `i64` |
| `float` | `f32` |
| `float64` | `f64` |
| `decimal` | `rust_decimal::Decimal` |
| `bool` | `bool` |
| `datetime` | `chrono::DateTime<Utc>` |
| `date` | `chrono::NaiveDate` |
| `time` | `chrono::NaiveTime` |
| `uuid` | `uuid::Uuid` |
| `bytes` | `Vec<u8>` |
| `json` | `serde_json::Value` |
| `T?` | `Option<T>` |
| `T[]` | `Vec<T>` |
| `map<K, V>` | `std::collections::HashMap<K, V>` |
| Custom Type | Generated `struct` |
| Enum | Generated `enum` |

### Schema to PostgreSQL

| Schema Type | PostgreSQL Type |
|-------------|-----------------|
| `string` | `TEXT` |
| `string(n)` | `VARCHAR(n)` |
| `int` | `INTEGER` |
| `int64` | `BIGINT` |
| `float` | `REAL` |
| `float64` | `DOUBLE PRECISION` |
| `decimal` | `NUMERIC` |
| `decimal @precision(p, s)` | `NUMERIC(p, s)` |
| `bool` | `BOOLEAN` |
| `datetime` | `TIMESTAMPTZ` |
| `date` | `DATE` |
| `time` | `TIME` |
| `uuid` | `UUID` |
| `bytes` | `BYTEA` |
| `json` / `jsonb` | `JSONB` |
| `T?` | Column allows `NULL` |
| `T[]` | `T[]` (PostgreSQL array) |
| `map<K, V>` | `JSONB` |
| Custom Type | `JSONB` or embedded columns |
| Enum | `TEXT` with CHECK or custom ENUM |

### Schema to Proto

| Schema Type | Proto Type |
|-------------|------------|
| `string` | `string` |
| `int` | `int32` |
| `int64` | `int64` |
| `uint` | `uint32` |
| `uint64` | `uint64` |
| `float` | `float` |
| `float64` | `double` |
| `decimal` | `string` (for precision) |
| `bool` | `bool` |
| `datetime` | `google.protobuf.Timestamp` |
| `duration` | `google.protobuf.Duration` |
| `bytes` | `bytes` |
| `json` | `google.protobuf.Struct` |
| `T?` | `optional T` |
| `T[]` | `repeated T` |
| `map<K, V>` | `map<K, V>` |
| Custom Type | Generated `message` |
| Enum | Generated `enum` |

---

## Nullability

### Required vs Optional

```yaml
fields:
  email:
    type: string                 # Required (NOT NULL)

  nickname:
    type: string?                # Optional (NULL allowed)
```

### Default Values

```yaml
fields:
  status:
    type: string
    attributes: ["@default(active)"]     # Has default, technically optional

  count:
    type: int
    attributes: ["@default(0)"]          # Has default

  created_at:
    type: datetime
    attributes: ["@default(now)"]        # Auto-generated
```

### Nullability Rules

1. **No `?`, no `@default`** → Required, must be provided
2. **Has `?`** → Optional, NULL allowed
3. **Has `@default`** → Optional at creation, default applied if not provided
4. **Has both** → Optional, NULL or default value

```yaml
fields:
  # Must provide at creation
  email:
    type: string
    attributes: ["@required"]

  # Optional, can be NULL
  nickname:
    type: string?

  # Optional at creation, defaults to 0
  login_count:
    type: int
    attributes: ["@default(0)"]

  # Can be NULL or defaults to "active"
  status:
    type: string?
    attributes: ["@default(active)"]
```

---

## Type Coercion

### Automatic Coercion

The schema system performs automatic type coercion where safe:

| Input | To Type | Result |
|-------|---------|--------|
| `"123"` | `int` | `123` |
| `123` | `string` | `"123"` |
| `"true"` | `bool` | `true` |
| `1` | `bool` | `true` |
| `0` | `bool` | `false` |

### Explicit Casting

In expressions (workflow rules, computed fields):

```yaml
computed:
  amount_string: "string(amount)"        # Cast decimal to string
  count_float: "float(count)"            # Cast int to float
  is_positive: "bool(amount > 0)"        # Expression to bool
```

---

## Validation Attributes Quick Reference

Complete list of all validation attributes organized by category.

### Size & Length
`@min(n)` `@max(n)` `@length(n)` `@range(min, max)` `@size(n)`

### String Format
`@email` `@url` `@phone` `@uuid` `@ulid` `@ip` `@ipv4` `@ipv6` `@mac_address` `@hex_color` `@json` `@ascii` `@slug`

### String Content
`@alpha` `@alpha_num` `@alpha_dash` `@lowercase` `@uppercase` `@pattern(regex)` `@not_pattern(regex)` `@starts_with(values)` `@ends_with(values)` `@doesnt_start_with(values)` `@doesnt_end_with(values)` `@contains(value)` `@doesnt_contain(value)`

### Numeric
`@integer` `@positive` `@negative` `@non_negative` `@non_positive` `@multiple_of(n)` `@digits(n)` `@digits_between(min, max)` `@decimal(places)` `@decimal_between(min, max)` `@precision(p, s)`

### Date & Time
`@after(date)` `@after_or_equal(date)` `@before(date)` `@before_or_equal(date)` `@date_format(format)` `@timezone` `@past` `@future`

### Enum & Choice
`@in(values)` `@not_in(values)` `@enum(EnumType)`

### Array
`@min_items(n)` `@max_items(n)` `@distinct` `@contains_all(values)` `@contains_any(values)`

### Cross-Field
`@same(field)` `@different(field)` `@gt(field)` `@gte(field)` `@lt(field)` `@lte(field)` `@required_with(fields)` `@required_without(fields)` `@required_if(field, value)` `@required_unless(field, value)` `@prohibited_if(field, value)` `@prohibited_unless(field, value)`

### Database
`@unique` `@unique_where(condition)` `@exists(table, column)` `@exists_where(condition)`

### Conditional
`@nullable` `@filled` `@present` `@sometimes` `@required`

### File & Image
`@file` `@image` `@mimes(types)` `@extensions(exts)` `@max_size(bytes)` `@min_size(bytes)` `@dimensions(constraints)` `@aspect_ratio(ratio)`

### Password
`@password` `@password_letters` `@password_mixed_case` `@password_numbers` `@password_symbols` `@password_uncompromised` `@current_password` `@confirmed`

### Financial
`@credit_card` `@iban` `@bic`

### Locale & Region
`@country_code` `@currency_code` `@language_code` `@locale` `@postal_code(country)`

### Network & Identifiers
`@domain` `@hostname` `@active_url` `@nik` `@npwp` `@isbn` `@issn` `@ean`

### Security
`@sensitive` `@encrypted` `@hashed`

### Documentation
`@description(text)` `@deprecated(reason)`

For detailed documentation of each attribute, see [RULE_FORMAT_MODELS.md — Field Attributes](./RULE_FORMAT_MODELS.md#field-attributes).

---

## Next Steps

- [RULE_FORMAT_MODELS.md](./RULE_FORMAT_MODELS.md) — entity definition syntax
- [RULE_FORMAT_HOOKS.md](./RULE_FORMAT_HOOKS.md) — entity rules, state machines, permissions
- [RULE_FORMAT_WORKFLOWS.md](./RULE_FORMAT_WORKFLOWS.md) — multi-step business processes
- [GENERATION.md](./GENERATION.md) - Code generation
- [EXAMPLES.md](./EXAMPLES.md) - Complete examples
