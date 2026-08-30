# Schema Standards

> **Consistency standards for Backbone schema definitions to ensure proper code generation and avoid common pitfalls.

## Table of Contents

1. [Model Standards](#model-standards)
2. [Field Standards](#field-standards)
3. [Type Standards](#type-standards)
4. [Validation Standards](#validation-standards)
5. [Common Patterns](#common-patterns)

---

## Model Standards

### Required Fields

All models MUST include these fields:

```yaml
fields:
  id:
    type: uuid
    attributes: ["@id", "@default(uuid)"]

  metadata:
    type: json
    attributes: ["@default('{}')"]
    description: "Metadata including timestamps and actors"
```

### Timestamp Fields (Optional)

If your model needs timestamps, use direct fields:

```yaml
  created_at:
    type: datetime
    attributes: ["@required", "@default(now)"]
    description: "Creation timestamp"

  updated_at:
    type: datetime?
    attributes: ["@updated_at()"]
    description: "Last update timestamp"

  deleted_at:
    type: datetime?
    description: "Soft delete timestamp (use metadata instead for new models)"
```

**NOTE:** For new models, prefer storing timestamps in `metadata` JSONB field rather than separate columns.

### Soft Delete Pattern

**Preferred (metadata-based):**
```yaml
metadata:
  type: json
  attributes: ["@default('{}')"]
  description: "Includes deleted_at, created_by, updated_at"
```

**Legacy (separate column):**
```yaml
deleted_at:
  type: datetime?
  description: "Soft delete timestamp"

is_deleted:
  type: bool
  attributes: ["@default(false)"]
  description: "Deletion flag"
```

---

## Field Standards

### Type Consistency

| Purpose | Type | Example |
|---------|------|---------|
| JSON metadata | `json` | `metadata: type: json` |
| Passwords | `string` with `@hashed` | `password_hash: type: string, @hashed` |
| Email addresses | `email` | `email: type: email` |
| Timestamps | `datetime` | `created_at: type: datetime` |
| Enums | Custom type | `status: type: UserStatus` |

### Common Field Patterns

**Password Hashing:**
```yaml
password_hash:
  type: string
  attributes: ["@required", "@min(130)", "@sensitive", "@hashed"]
  description: "Argon2id hash"
```

**Foreign Keys:**
```yaml
user_id:
  type: uuid
  attributes: ["@required", "@foreign_key(User.id)"]
  description: "Reference to User"
```

**Boolean Defaults:**
```yaml
is_active:
  type: bool
  attributes: ["@default(true)"]
```

---

## Type Standards

### Use `json` for Metadata Fields

**CORRECT:**
```yaml
metadata:
  type: json
  attributes: ["@default('{}')"]
```

**INCORRECT:**
```yaml
metadata:
  type: Metadata  # No such type defined
```

### Enum Definition

All enums MUST have at least one variant marked as default:

```yaml
enums:
  - name: UserStatus
    variants:
      - name: active
        default: true
      - name: inactive
      - name: suspended
```

---

## Validation Standards

### Foreign Key Attributes

All foreign key fields MUST have `@foreign_key` attribute:

```yaml
user_id:
  type: uuid
  attributes: ["@required", "@foreign_key(User.id)"]
```

### Required Fields

Use `@required` for non-nullable fields:

```yaml
email:
  type: email
  attributes: ["@required", "@unique"]
```

### String Validation

Use appropriate validation attributes:

```yaml
username:
  type: string
  attributes: ["@required", "@min(3)", "@max(50)", "@alpha_dash"]
```

---

## Common Patterns

### Permission Grant Models

Choose the appropriate model based on complexity:

**Simple Permission Grants (UserRole):**
- Basic many-to-many between users and roles
- No expiration, scoping, or conditions
- Use for permanent role assignments

**Complex Permission Grants (RoleAssignment):**
- Expiration dates
- Resource scoping
- Priority and conflict resolution
- Usage tracking

**Direct Permission Grants (UserPermission):**
- Exception-based permissions
- Resource-specific permissions
- Temporary access with audit trail

**Advanced Permission Grants (DirectPermissionGrant):**
- All features of UserPermission
- Usage tracking (last_used_at, usage_count)
- Conditions and access logging

### Password-Related Models

Keep separate concerns:

- `password_policy`: Rules for password requirements
- `password_history`: Previous password hashes (prevent reuse)
- `password_reset_token`: Reset tokens
- `password_reset`: Reset request tracking

### Audit Fields

For models that need audit trail:

```yaml
created_at:
  type: datetime
  attributes: ["@default(now)"]

created_by:
  type: uuid
  attributes: ["@foreign_key(User.id)"]

updated_at:
  type: datetime?

updated_by:
  type: uuid?
  attributes: ["@foreign_key(User.id)"]

deleted_at:
  type: datetime?

deleted_by:
  type: uuid?
  attributes: ["@foreign_key(User.id)"]
```

Or use metadata pattern (preferred):

```yaml
metadata:
  type: json
  attributes: ["@default('{}')"]
  description: "Includes created_at, created_by, updated_at, updated_by, deleted_at, deleted_by"
```

---

## Model Naming Conventions

| Pattern | Use | Example |
|---------|-----|---------|
| `{Entity}` | Primary entity | `User`, `Role` |
| `{Entity}{Qualifier}` | Qualified entity | `UserRole`, `Permission` |
| `{Action}{Entity}` | Action-based entity | `PasswordReset`, `EmailVerification` |
| `{Entity}{Concept}` | Concept-based | `SecurityEvent`, `AuditLog` |

**Avoid duplicates:** If two models serve similar purposes, consolidate or clearly differentiate their use cases.
