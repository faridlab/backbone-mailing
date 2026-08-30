# OpenAPI Schema Generation

This document describes how OpenAPI specifications are generated from model and workflow schemas, and how to customize REST API definitions.

---

## Overview

The schema system generates OpenAPI 3.0 specifications alongside gRPC proto files, enabling:

- **REST API documentation** from the same schema source
- **Client SDK generation** for multiple languages
- **API testing tools** integration (Postman, Insomnia)
- **API gateway configuration** (Kong, AWS API Gateway)

```
*.model.yaml + *.workflow.yaml
              ↓
    backbone-schema schema generate <module> --target openapi
              ↓
┌─────────────────────────────────────────┐
│ Generated Outputs:                       │
│ ✓ openapi.yaml (complete spec)          │
│ ✓ Per-entity endpoint definitions        │
│ ✓ Request/response schemas               │
│ ✓ Authentication requirements            │
│ ✓ Validation constraints                 │
└─────────────────────────────────────────┘
```

---

## Generation Strategy

### Non-Destructive Updates (Git-Aware)

The OpenAPI generator uses **git-aware incremental updates** to preserve hand-written customizations:

```
┌─────────────────────────────────────────────────────────────┐
│  1. READ EXISTING                                            │
│     - Load existing openapi.yaml (if exists)                │
│     - Parse into structured sections                         │
│     - Identify generated vs hand-written sections            │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│  2. GENERATE FROM SCHEMA                                     │
│     - Generate paths from models                             │
│     - Generate schemas from entities                         │
│     - Generate security from permissions                     │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│  3. SMART MERGE                                              │
│     - Compare generated vs existing                          │
│     - Preserve hand-written extensions                       │
│     - Update only schema-driven sections                     │
│     - Mark conflicts for manual resolution                   │
└─────────────────────────────────────────────────────────────┘
                              ↓
┌─────────────────────────────────────────────────────────────┐
│  4. OUTPUT                                                   │
│     - Write merged openapi.yaml                              │
│     - Generate diff report                                   │
│     - List preserved customizations                          │
└─────────────────────────────────────────────────────────────┘
```

### Section Markers

Generated sections are marked to distinguish from hand-written code:

```yaml
# openapi.yaml

openapi: 3.0.3
info:
  title: Sapiens API
  version: 1.0.0
  # [CUSTOM] - Hand-written, preserved during regeneration
  description: |
    User management and authentication API.

    ## Authentication
    All endpoints require Bearer token authentication.

paths:
  # [GENERATED:users] - Auto-generated from user.model.yaml
  /api/v1/users:
    get:
      summary: List users
      # ... generated content
    post:
      summary: Create user
      # ... generated content

  # [CUSTOM] - Hand-written endpoint, preserved
  /api/v1/users/me:
    get:
      summary: Get current user
      # ... custom content

components:
  schemas:
    # [GENERATED:User] - Auto-generated from user.model.yaml
    User:
      type: object
      properties:
        # ... generated properties

    # [CUSTOM] - Hand-written schema
    UserPreferences:
      type: object
      # ... custom content
```

---

## CLI Commands

### Generate OpenAPI

```bash
# Generate OpenAPI for a module
backbone-cli schema:generate sapiens --target openapi

# Generate with diff preview (don't write)
backbone-cli schema:generate sapiens --target openapi --dry-run

# Force overwrite (ignore existing customizations)
backbone-cli schema:generate sapiens --target openapi --force

# Generate to specific output location
backbone-cli schema:generate sapiens --target openapi --output ./api/openapi.yaml
```

### Diff OpenAPI

```bash
# Show what would change in existing OpenAPI
backbone-cli schema:diff sapiens --target openapi

# Output:
# Modified: /api/v1/users (added field: avatar_url)
# Modified: User schema (added property: avatar_url)
# Preserved: /api/v1/users/me (custom endpoint)
# Preserved: UserPreferences schema (custom)
```

### Validate OpenAPI

```bash
# Validate generated OpenAPI against schema
backbone-cli schema:validate sapiens --target openapi

# Check for:
# - Missing endpoints for entities
# - Schema mismatches
# - Outdated generated sections
```

---

## Generated Structure

### From Model to OpenAPI

**Input: user.model.yaml**
```yaml
models:
  - name: User
    collection: users

    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]

      email:
        type: email
        attributes: ["@unique", "@required"]

      username:
        type: string
        attributes: ["@min(3)", "@max(50)", "@required"]

      status:
        type: UserStatus
        attributes: ["@default(active)"]

      created_at:
        type: datetime
        attributes: ["@default(now)"]

      deleted_at:
        type: datetime?

enums:
  - name: UserStatus
    values:
      - active
      - inactive
      - suspended
```

**Output: openapi.yaml (paths section)**
```yaml
paths:
  # [GENERATED:users]
  /api/v1/users:
    get:
      operationId: listUsers
      summary: List users
      description: Retrieve a paginated list of users
      tags:
        - Users
      parameters:
        - $ref: '#/components/parameters/PageParam'
        - $ref: '#/components/parameters/LimitParam'
        - $ref: '#/components/parameters/SortParam'
        - $ref: '#/components/parameters/FilterParam'
        - name: status
          in: query
          description: Filter by status
          schema:
            $ref: '#/components/schemas/UserStatus'
      responses:
        '200':
          description: Successful response
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/UserListResponse'
        '401':
          $ref: '#/components/responses/Unauthorized'
        '403':
          $ref: '#/components/responses/Forbidden'
      security:
        - bearerAuth: []

    post:
      operationId: createUser
      summary: Create user
      description: Create a new user
      tags:
        - Users
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateUserRequest'
      responses:
        '201':
          description: User created
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
        '400':
          $ref: '#/components/responses/ValidationError'
        '401':
          $ref: '#/components/responses/Unauthorized'
        '409':
          $ref: '#/components/responses/Conflict'
      security:
        - bearerAuth: []

  /api/v1/users/{id}:
    get:
      operationId: getUser
      summary: Get user by ID
      tags:
        - Users
      parameters:
        - $ref: '#/components/parameters/IdParam'
      responses:
        '200':
          description: Successful response
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
        '404':
          $ref: '#/components/responses/NotFound'
      security:
        - bearerAuth: []

    put:
      operationId: updateUser
      summary: Update user
      tags:
        - Users
      parameters:
        - $ref: '#/components/parameters/IdParam'
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/UpdateUserRequest'
      responses:
        '200':
          description: User updated
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
        '400':
          $ref: '#/components/responses/ValidationError'
        '404':
          $ref: '#/components/responses/NotFound'
      security:
        - bearerAuth: []

    patch:
      operationId: partialUpdateUser
      summary: Partial update user
      tags:
        - Users
      parameters:
        - $ref: '#/components/parameters/IdParam'
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/PatchUserRequest'
      responses:
        '200':
          description: User updated
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
        '400':
          $ref: '#/components/responses/ValidationError'
        '404':
          $ref: '#/components/responses/NotFound'
      security:
        - bearerAuth: []

    delete:
      operationId: deleteUser
      summary: Delete user (soft delete)
      tags:
        - Users
      parameters:
        - $ref: '#/components/parameters/IdParam'
      responses:
        '204':
          description: User deleted
        '404':
          $ref: '#/components/responses/NotFound'
      security:
        - bearerAuth: []

  /api/v1/users/bulk:
    post:
      operationId: bulkCreateUsers
      summary: Bulk create users
      tags:
        - Users
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/BulkCreateUsersRequest'
      responses:
        '201':
          description: Users created
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/BulkCreateUsersResponse'
      security:
        - bearerAuth: []

  /api/v1/users/upsert:
    post:
      operationId: upsertUser
      summary: Upsert user
      tags:
        - Users
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/UpsertUserRequest'
      responses:
        '200':
          description: User upserted
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
      security:
        - bearerAuth: []

  /api/v1/users/trash:
    get:
      operationId: listDeletedUsers
      summary: List deleted users
      tags:
        - Users
      parameters:
        - $ref: '#/components/parameters/PageParam'
        - $ref: '#/components/parameters/LimitParam'
      responses:
        '200':
          description: Successful response
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/UserListResponse'
      security:
        - bearerAuth: []

  /api/v1/users/{id}/restore:
    post:
      operationId: restoreUser
      summary: Restore deleted user
      tags:
        - Users
      parameters:
        - $ref: '#/components/parameters/IdParam'
      responses:
        '200':
          description: User restored
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
        '404':
          $ref: '#/components/responses/NotFound'
      security:
        - bearerAuth: []

  /api/v1/users/empty:
    delete:
      operationId: emptyUsersTrash
      summary: Empty trash (permanent delete)
      tags:
        - Users
      responses:
        '204':
          description: Trash emptied
      security:
        - bearerAuth: []
```

**Output: openapi.yaml (schemas section)**
```yaml
components:
  schemas:
    # [GENERATED:User]
    User:
      type: object
      required:
        - id
        - email
        - username
        - status
        - created_at
      properties:
        id:
          type: string
          format: uuid
          readOnly: true
          description: Unique identifier
        email:
          type: string
          format: email
          description: User email address
        username:
          type: string
          minLength: 3
          maxLength: 50
          description: Username
        status:
          $ref: '#/components/schemas/UserStatus'
        created_at:
          type: string
          format: date-time
          readOnly: true
        deleted_at:
          type: string
          format: date-time
          nullable: true
          readOnly: true

    # [GENERATED:UserStatus]
    UserStatus:
      type: string
      enum:
        - active
        - inactive
        - suspended
      default: active

    # [GENERATED:CreateUserRequest]
    CreateUserRequest:
      type: object
      required:
        - email
        - username
      properties:
        email:
          type: string
          format: email
        username:
          type: string
          minLength: 3
          maxLength: 50
        status:
          $ref: '#/components/schemas/UserStatus'

    # [GENERATED:UpdateUserRequest]
    UpdateUserRequest:
      type: object
      required:
        - email
        - username
        - status
      properties:
        email:
          type: string
          format: email
        username:
          type: string
          minLength: 3
          maxLength: 50
        status:
          $ref: '#/components/schemas/UserStatus'

    # [GENERATED:PatchUserRequest]
    PatchUserRequest:
      type: object
      properties:
        email:
          type: string
          format: email
        username:
          type: string
          minLength: 3
          maxLength: 50
        status:
          $ref: '#/components/schemas/UserStatus'

    # [GENERATED:UserListResponse]
    UserListResponse:
      type: object
      properties:
        data:
          type: array
          items:
            $ref: '#/components/schemas/User'
        meta:
          $ref: '#/components/schemas/PaginationMeta'
```

---

## Workflow Integration

### State Transitions as Endpoints

**Input: user.workflow.yaml**
```yaml
states:
  field: status

  transitions:
    verify:
      from: pending_verification
      to: active
      roles: [system]

    suspend:
      from: [active, inactive]
      to: suspended
      roles: [admin]

    reactivate:
      from: suspended
      to: active
      roles: [admin]
```

**Output: Transition endpoints**
```yaml
paths:
  # [GENERATED:users:transitions]
  /api/v1/users/{id}/verify:
    post:
      operationId: verifyUser
      summary: Verify user
      description: |
        Transition user status from `pending_verification` to `active`.

        **Allowed roles:** system
      tags:
        - Users
        - State Transitions
      parameters:
        - $ref: '#/components/parameters/IdParam'
      responses:
        '200':
          description: User verified
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
        '400':
          description: Invalid transition
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/TransitionError'
        '404':
          $ref: '#/components/responses/NotFound'
      security:
        - bearerAuth: []

  /api/v1/users/{id}/suspend:
    post:
      operationId: suspendUser
      summary: Suspend user
      description: |
        Transition user status from `active` or `inactive` to `suspended`.

        **Allowed roles:** admin
      tags:
        - Users
        - State Transitions
      parameters:
        - $ref: '#/components/parameters/IdParam'
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                reason:
                  type: string
                  description: Reason for suspension
      responses:
        '200':
          description: User suspended
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
        '400':
          $ref: '#/components/responses/InvalidTransition'
      security:
        - bearerAuth: []

  /api/v1/users/{id}/reactivate:
    post:
      operationId: reactivateUser
      summary: Reactivate user
      description: |
        Transition user status from `suspended` to `active`.

        **Allowed roles:** admin
      tags:
        - Users
        - State Transitions
      parameters:
        - $ref: '#/components/parameters/IdParam'
      responses:
        '200':
          description: User reactivated
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/User'
        '400':
          $ref: '#/components/responses/InvalidTransition'
      security:
        - bearerAuth: []
```

### Permissions as Security Requirements

**Input: user.workflow.yaml**
```yaml
permissions:
  admin:
    allow:
      - all

  user:
    allow:
      - action: read
        if: "id == $actor.id"
      - action: update
        if: "id == $actor.id"
        only: [email, username]
    deny:
      - delete
```

**Output: Security schemes and role documentation**
```yaml
components:
  securitySchemes:
    bearerAuth:
      type: http
      scheme: bearer
      bearerFormat: JWT
      description: |
        JWT token authentication.

        Include the token in the Authorization header:
        ```
        Authorization: Bearer <token>
        ```

  # Role-based access documentation
  x-roles:
    admin:
      description: Full access to all operations
      permissions:
        - all

    user:
      description: Limited access to own resources
      permissions:
        - read (own)
        - update (own, fields: email, username)
      restrictions:
        - Cannot delete

# Endpoint-specific security notes
paths:
  /api/v1/users/{id}:
    get:
      x-permissions:
        admin: "Full access"
        user: "Only if id == actor.id"
```

---

## Validation Rules in OpenAPI

**Input: user.workflow.yaml**
```yaml
rules:
  username_format:
    condition: "username matches '^[a-z0-9_]{3,50}$'"
    message: "Username must be 3-50 lowercase alphanumeric"
    code: INVALID_USERNAME

  password_strength:
    condition: |
      password.length >= 8 &&
      password matches '[A-Z]' &&
      password matches '[a-z]' &&
      password matches '[0-9]'
    message: "Password must be 8+ characters with uppercase, lowercase, and number"
    code: WEAK_PASSWORD
```

**Output: Schema with validation**
```yaml
components:
  schemas:
    CreateUserRequest:
      type: object
      properties:
        username:
          type: string
          pattern: '^[a-z0-9_]{3,50}$'
          description: |
            Username must be 3-50 lowercase alphanumeric.

            **Validation code:** INVALID_USERNAME

        password:
          type: string
          minLength: 8
          description: |
            Password must be 8+ characters with uppercase, lowercase, and number.

            **Validation code:** WEAK_PASSWORD
          x-validation:
            - pattern: '[A-Z]'
              message: Must contain uppercase letter
            - pattern: '[a-z]'
              message: Must contain lowercase letter
            - pattern: '[0-9]'
              message: Must contain number
```

---

## Customization Options

### OpenAPI Overrides File

Create `*.openapi.yaml` files to customize generated OpenAPI:

```yaml
# libs/modules/sapiens/schema/openapi/user.openapi.yaml

# Override or extend generated OpenAPI for User
overrides:
  # Custom info section
  info:
    description: |
      Extended description for User API.

      ## Rate Limits
      - 100 requests per minute for read operations
      - 10 requests per minute for write operations

  # Override specific paths
  paths:
    /api/v1/users:
      get:
        # Add custom parameter
        parameters:
          - name: include_deleted
            in: query
            schema:
              type: boolean
            description: Include soft-deleted users

        # Override responses
        responses:
          '200':
            description: Custom description
            headers:
              X-Rate-Limit-Remaining:
                schema:
                  type: integer

  # Add custom endpoints (preserved during regeneration)
  custom_paths:
    /api/v1/users/me:
      get:
        operationId: getCurrentUser
        summary: Get current authenticated user
        tags:
          - Users
        responses:
          '200':
            content:
              application/json:
                schema:
                  $ref: '#/components/schemas/User'

    /api/v1/users/search:
      post:
        operationId: searchUsers
        summary: Advanced user search
        requestBody:
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/UserSearchRequest'

  # Add custom schemas
  custom_schemas:
    UserSearchRequest:
      type: object
      properties:
        query:
          type: string
        filters:
          type: object
          additionalProperties: true
```

### Module-Level OpenAPI Config

In `index.model.yaml`:

```yaml
module: sapiens
version: 1

openapi:
  title: Sapiens User Management API
  version: 1.0.0
  description: User management, authentication, and authorization

  servers:
    - url: https://api.example.com
      description: Production
    - url: https://staging-api.example.com
      description: Staging
    - url: http://localhost:3001
      description: Local development

  tags:
    - name: Users
      description: User management operations
    - name: Authentication
      description: Login, logout, and token management
    - name: Roles
      description: Role and permission management

  security:
    default: bearerAuth

  responses:
    include_rate_limit_headers: true
    include_request_id: true
```

---

## Merge Strategies

### What Gets Preserved

| Section | Behavior |
|---------|----------|
| `info.description` | Preserved if marked `[CUSTOM]` |
| `info.contact`, `info.license` | Always preserved |
| `servers` | Merged (custom servers added) |
| `paths` (generated entities) | Updated from schema |
| `paths` (custom endpoints) | Preserved |
| `components/schemas` (generated) | Updated from schema |
| `components/schemas` (custom) | Preserved |
| `components/securitySchemes` | Merged |
| `x-*` extensions | Preserved |

### What Gets Updated

- Entity CRUD paths (`/api/v1/{collection}/*`)
- Entity schemas (`User`, `CreateUserRequest`, etc.)
- Enum schemas
- Transition endpoints (from workflow)
- Validation patterns and constraints

### Conflict Resolution

When a conflict is detected:

```bash
$ backbone-cli schema:generate sapiens --target openapi

⚠ Conflict detected in /api/v1/users:
  - Schema says: email is required
  - Existing says: email is optional

Options:
  1. Use schema definition (recommended)
  2. Keep existing (manual override)
  3. Skip this endpoint

Enter choice [1/2/3]:
```

Or use flags:

```bash
# Always use schema definition
backbone-cli schema:generate sapiens --target openapi --strategy schema-wins

# Always preserve existing
backbone-cli schema:generate sapiens --target openapi --strategy existing-wins

# Generate conflict markers
backbone-cli schema:generate sapiens --target openapi --strategy mark-conflicts
```

---

## Type Mapping

### Schema Type to OpenAPI Type

| Schema Type | OpenAPI Type | Format | Additional |
|-------------|--------------|--------|------------|
| `uuid` | `string` | `uuid` | |
| `string` | `string` | | |
| `int` | `integer` | `int32` | |
| `int64` | `integer` | `int64` | |
| `float` | `number` | `float` | |
| `float64` | `number` | `double` | |
| `bool` | `boolean` | | |
| `datetime` | `string` | `date-time` | |
| `date` | `string` | `date` | |
| `time` | `string` | `time` | |
| `email` | `string` | `email` | |
| `url` | `string` | `uri` | |
| `phone` | `string` | | `pattern: ^\+?[1-9]\d{1,14}$` |
| `ip` | `string` | | `oneOf: [ipv4, ipv6]` |
| `json` | `object` | | `additionalProperties: true` |
| `decimal` | `string` | | `pattern: ^\d+\.\d+$` |
| `money` | `object` | | `{amount, currency}` |
| `bytes` | `string` | `byte` | |
| `Type?` | | | `nullable: true` |
| `Type[]` | `array` | | `items: {$ref}` |

### Attribute to OpenAPI Constraint

| Attribute | OpenAPI Property |
|-----------|------------------|
| `@required` | `required: [field]` |
| `@min(n)` | `minLength: n` (string) or `minimum: n` (number) |
| `@max(n)` | `maxLength: n` (string) or `maximum: n` (number) |
| `@unique` | (documented in description) |
| `@default(v)` | `default: v` |
| `@pattern(regex)` | `pattern: regex` |
| `@email` | `format: email` |
| `@url` | `format: uri` |
| `@sensitive` | (excluded from response, `writeOnly: true`) |

---

## Output Files

```
libs/modules/{module}/
├── schema/
│   ├── models/
│   ├── workflows/
│   └── openapi/                    # OpenAPI customizations
│       ├── index.openapi.yaml      # Module-level config
│       └── {entity}.openapi.yaml   # Per-entity overrides
│
└── openapi/                        # Generated OpenAPI
    ├── openapi.yaml                # Complete spec (single file)
    ├── paths/                      # Split paths (optional)
    │   ├── users.yaml
    │   └── roles.yaml
    └── schemas/                    # Split schemas (optional)
        ├── user.yaml
        └── role.yaml
```

---

## Integration Examples

### Swagger UI

```yaml
# docker-compose.yml
services:
  swagger-ui:
    image: swaggerapi/swagger-ui
    ports:
      - "8080:8080"
    environment:
      SWAGGER_JSON: /openapi/openapi.yaml
    volumes:
      - ./libs/modules/sapiens/openapi:/openapi
```

### API Gateway (Kong)

```bash
# Import OpenAPI to Kong
deck sync --kong-addr http://localhost:8001 \
  --openapi-spec libs/modules/sapiens/openapi/openapi.yaml
```

### Client SDK Generation

```bash
# Generate TypeScript client
openapi-generator generate \
  -i libs/modules/sapiens/openapi/openapi.yaml \
  -g typescript-axios \
  -o clients/typescript

# Generate Rust client
openapi-generator generate \
  -i libs/modules/sapiens/openapi/openapi.yaml \
  -g rust \
  -o clients/rust
```

---

## Best Practices

### 1. Keep Custom Endpoints Separate

```yaml
# Good: Use openapi/ override files
schema/openapi/user.openapi.yaml:
  custom_paths:
    /api/v1/users/me: ...

# Avoid: Mixing custom with generated
# (will be overwritten on regeneration)
```

### 2. Use Descriptive Operation IDs

Operation IDs are auto-generated but can be overridden:

```yaml
# Generated: listUsers, createUser, getUser
# Override in openapi file if needed:
overrides:
  paths:
    /api/v1/users:
      get:
        operationId: getAllUsers  # Custom ID
```

### 3. Document Business Rules

Add `x-business-rules` extension for complex validations:

```yaml
components:
  schemas:
    CreateOrderRequest:
      x-business-rules:
        - name: minimum_order
          description: Order total must be at least $10
          code: MINIMUM_ORDER_AMOUNT
        - name: items_in_stock
          description: All items must be in stock
          code: ITEMS_OUT_OF_STOCK
```

### 4. Version Your API

```yaml
# index.model.yaml
openapi:
  version: 2.0.0  # Bump when breaking changes

# paths use version prefix
paths:
  /api/v2/users: ...  # New version
  /api/v1/users: ...  # Deprecated
```

---

## Next Steps

- [RULE_FORMAT_MODELS.md](./RULE_FORMAT_MODELS.md) — entity definition syntax
- [RULE_FORMAT_HOOKS.md](./RULE_FORMAT_HOOKS.md) — state machines, validation, permissions
- [RULE_FORMAT_WORKFLOWS.md](./RULE_FORMAT_WORKFLOWS.md) — multi-step business processes
- [GENERATION.md](./GENERATION.md) — generator targets and CLI
- [GENERATION.md](./GENERATION.md) - All code generation targets
