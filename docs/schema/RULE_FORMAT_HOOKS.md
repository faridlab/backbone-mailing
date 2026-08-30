# Hook Schema YAML Rules & Format

> **CRITICAL**: Follow these rules exactly to prevent validation errors and code generation failures.

This document is the **canonical authoring reference** for `*.hook.yaml` files. It is the merger of the previous HOOK.md, AUTHORIZATION.md, and the entity-rule portion of VALIDATION_RULES.md. Hooks describe entity lifecycle behavior — state machines, validation rules, RBAC/ABAC permissions, triggers, and computed fields.

> **Generator note (Phase 2 enforcement)**: State machines defined here are **enforced** at the entity layer through the `StateMachineBehavior` trait (extracted to `backbone-core` in Phase 8). The entity generator emits state-validation guards on `update()` calls — illegal transitions become compile-time obvious and runtime errors. You no longer need to write manual transition guards.

## Table of Contents

1. [Overview](#overview)
2. [File Organization](#file-organization)
3. [Hook File Structure](#hook-file-structure)
4. [State Machines](#state-machines)
5. [State Actions](#state-actions)
6. [Transitions](#transitions)
7. [Validation Rules](#validation-rules)
8. [Permissions (RBAC)](#permissions-rbac)
9. [Attribute-Based Access Control (ABAC)](#attribute-based-access-control-abac)
10. [Triggers](#triggers)
11. [Computed Fields](#computed-fields)
12. [Expression Syntax](#expression-syntax)
13. [Common Mistakes](#common-mistakes)

---

## Overview

Hooks define **entity lifecycle behaviors**:
- **State Machines**: Status field transitions with guards and actions
- **Rules**: Validation constraints for create/update operations
- **Permissions**: Role-based access control (RBAC)
- **Triggers**: Side effects on lifecycle events
- **Computed**: Read-only calculated fields

---

## File Organization

```
libs/modules/{module}/schema/hooks/
├── index.hook.yaml            # Module-level events, scheduled jobs
├── {entity}.hook.yaml         # Entity lifecycle hooks
└── ...
```

### Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| File names | `snake_case.hook.yaml` | `stored_file.hook.yaml` |
| Hook name | `PascalCase` (matches model) | `StoredFile` |
| State names | `snake_case` | `pending`, `in_progress` |
| Transition names | `snake_case` | `approve`, `mark_complete` |
| Rule names | `snake_case` | `valid_email`, `path_length` |
| Trigger names | `snake_case` | `after_create`, `before_update` |

---

## Hook File Structure

### Complete Template

```yaml
# Entity name (must match model name)
name: EntityName
model: entity_name.model.yaml

# State machine definition
states:
  field: status                   # Field that holds the state
  values:
    state_name:
      initial: true               # Exactly ONE state must be initial
      final: false                # Terminal states
      on_enter: [actions]         # Actions when entering state
      on_exit: [actions]          # Actions when leaving state

  transitions:
    transition_name:
      from: state_name            # Single state or [array]
      to: target_state            # Single target state
      roles: [role1, role2]       # Required roles
      condition: "expression"     # Guard condition
      message: "Error message"    # Shown if condition fails

# Validation rules
rules:
  rule_name:
    when: [create, update]        # When to apply
    condition: "expression"       # Must be true
    message: "Error message"      # User-facing error
    code: ERROR_CODE              # Optional error code

# Access control
permissions:
  role_name:
    allow:
      - action: action_name
        if: "condition"           # Optional condition
        only: [field1, field2]    # Optional field restriction
    deny:
      - action_name

# Lifecycle triggers
triggers:
  trigger_name:
    actions: [action_list]
    if: "condition"               # Optional condition

# Computed fields
computed:
  field_name: "expression"
```

---

## State Machines

### Basic Structure

```yaml
states:
  field: status                   # REQUIRED: Field that holds state value

  values:                         # REQUIRED: Define all possible states
    draft:
      initial: true               # REQUIRED: Exactly ONE initial state
    pending:
      on_enter:
        - "log(order.pending, { id: id })"
    approved:
      on_enter:
        - "emit: OrderApprovedEvent(order_id: id)"
    rejected:
      final: true                 # Optional: Mark as terminal state
    completed:
      final: true
      on_enter:
        - "emit: OrderCompletedEvent(order_id: id)"

  transitions:                    # Define allowed state changes
    submit:
      from: draft
      to: pending
    approve:
      from: pending
      to: approved
      roles: [admin, manager]
    reject:
      from: pending
      to: rejected
      roles: [admin, manager]
    complete:
      from: approved
      to: completed
      condition: "items.length > 0"
```

### State Definition

```yaml
values:
  state_name:
    # Mark as initial state (REQUIRED: exactly one)
    initial: true

    # Mark as final/terminal state
    final: true

    # Actions on entering this state
    on_enter:
      - "action1"
      - "action2"

    # Actions on exiting this state
    on_exit:
      - "action1"
```

### Critical Rules for States

1. **Exactly ONE state must have `initial: true`**
2. **State names must be `snake_case`**
3. **Field must be an enum type in the model**
4. **All enum variants should have corresponding state definitions**

---

## State Actions

Actions executed when entering or exiting a state.

### Action Types

```yaml
on_enter:
  # Logging
  - "log(event_name, { key: value })"

  # Emit domain event
  - "emit: EventName(field: value, field2: $this.id)"

  # Send notification
  - "notify([admin, owner], Message: #{field_name})"

  # Send email
  - "send_email(template_name, recipient_email, { data: value })"

  # Update field
  - "set: field_name = value"
  - "set: metadata.updated_at = now()"

  # Call function/trigger
  - "trigger: function_name"

  # Queue background job
  - "queue: JobName(param: value)"

  # Update related entity
  - "update: Entity(id: related_id, field: new_value)"

  # Create related entity
  - "create: Entity(field1: value1, field2: value2)"

  # Delete related entities
  - "delete: Entity(condition)"

  # Delete storage
  - "delete_storage: storage_key_field"
```

### Conditional Actions

```yaml
on_enter:
  # Action with condition
  - "send_email(alert, admin@example.com, { id: id }) if: threat_level == 'high'"

  # Conditional emit
  - "emit: HighPriorityEvent(id: id) if: priority == 'critical'"
```

### Action Expression Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `id` | Entity ID | `id` |
| `$this` | Current entity | `$this.owner_id` |
| `$old` | Previous values | `$old.status` |
| `$actor` | Current user | `$actor.id` |
| `$context` | Workflow context | `$context.threat_name` |
| Field names | Direct field access | `owner_id`, `status` |

---

## Transitions

### Basic Transition

```yaml
transitions:
  transition_name:
    from: source_state            # Required
    to: target_state              # Required
```

### Transition with Guards

```yaml
transitions:
  approve:
    from: pending
    to: approved
    roles: [admin, manager]       # Who can perform
    condition: "amount <= 10000"  # Guard condition
    message: "Amount exceeds approval limit"
```

### Multiple Source States

```yaml
transitions:
  soft_delete:
    from: [active, suspended]     # From multiple states
    to: deleted
    roles: [admin, owner]
    condition: "$actor.id == owner_id || $actor.has_role('admin')"
```

### Transition Condition Expressions

```yaml
transitions:
  complete_upload:
    from: uploading
    to: processing
    condition: "size_bytes > 0"

  mark_safe:
    from: processing
    to: active
    condition: "is_scanned == true && (threat_level == null || threat_level == 'safe')"

  restore:
    from: deleted
    to: active
    condition: "metadata.deleted_at != null && ($actor.id == owner_id || $actor.has_role('admin'))"

  purge:
    from: deleted
    to: purged
    condition: "metadata.deleted_at != null && metadata.deleted_at < now() - 30.days"
```

---

## Validation Rules

### Rule Structure

```yaml
rules:
  rule_name:
    when: [create, update]        # When to apply (default: always)
    condition: "expression"       # Must evaluate to true
    message: "User-facing error"  # Error message
    code: ERROR_CODE              # Optional error code
```

### When Clauses

```yaml
rules:
  # Apply on create only
  create_only_rule:
    when: [create]
    condition: "expression"

  # Apply on update only
  update_only_rule:
    when: [update]
    condition: "expression"

  # Apply on both (default if omitted)
  always_rule:
    when: [create, update]
    condition: "expression"

  # Shorthand: omit 'when' for always
  simple_rule:
    condition: "expression"
    message: "Error"
```

### Rule Examples

```yaml
rules:
  # String length validation
  path_length:
    when: [create, update]
    condition: "path.length <= 1024"
    message: "File path too long (max 1024 characters)"
    code: PATH_TOO_LONG

  # Pattern validation
  no_path_traversal:
    when: [create, update]
    condition: "!path.contains('..')"
    message: "Path traversal not allowed"
    code: PATH_TRAVERSAL

  # Regex validation
  valid_mime_type:
    when: [create]
    condition: "mime_type matches '^[a-z]+/[a-z0-9.+-]+$'"
    message: "Invalid MIME type format"
    code: INVALID_MIME_TYPE

  # Status-based validation
  file_not_purged:
    when: [update]
    condition: "status != 'purged'"
    message: "Cannot update purged files"
    code: FILE_PURGED

  # Role-based validation
  quarantine_admin_only:
    when: [update]
    condition: "status != 'quarantined' || $actor.has_role('admin')"
    message: "Only admins can modify quarantined files"
    code: QUARANTINE_RESTRICTED

  # Cross-field validation
  end_after_start:
    condition: "end_date > start_date"
    message: "End date must be after start date"
    code: INVALID_DATE_RANGE

  # Referential validation
  valid_owner:
    when: [create]
    condition: "exists(users, id, owner_id)"
    message: "Owner does not exist"
    code: INVALID_OWNER

  # Comparison with old value
  no_status_downgrade:
    when: [update]
    condition: "status_level($this.status) >= status_level($old.status)"
    message: "Cannot downgrade status"
```

---

## Permissions (RBAC)

### Structure

```yaml
permissions:
  role_name:
    allow:
      - action_name               # Simple allow
      - action: action_name       # Allow with conditions
        if: "condition"
        only: [field1, field2]
    deny:
      - action_name               # Explicit deny
```

### Permission Actions

| Action | Description |
|--------|-------------|
| `all` | Full access to all actions |
| `read` | View entity |
| `create` | Create new entity |
| `update` | Modify entity |
| `delete` | Delete entity |
| `list` | List/query entities |
| `{custom}` | Custom action (e.g., `download`, `share`, `restore`) |

### Complete Example

```yaml
permissions:
  # Super admin - full access
  super_admin:
    allow:
      - all

  # Admin - broad access
  admin:
    allow:
      - action: read
      - action: update
      - action: delete
      - action: list
      - action: download
      - action: restore
      - action: quarantine
      - action: mark_safe

  # Regular user - conditional access
  user:
    allow:
      - action: create
      - action: read
        if: "$this.owner_id == $actor.id || has_share_access($actor.id, $this.id)"
      - action: update
        if: "$this.owner_id == $actor.id"
        only: [name, description, tags]    # Field-level restriction
      - action: delete
        if: "$this.owner_id == $actor.id"
      - action: download
        if: "$this.owner_id == $actor.id || has_share_access($actor.id, $this.id)"
      - action: list
        if: "$this.owner_id == $actor.id"
    deny:
      - action: quarantine
      - action: mark_safe

  # Guest - minimal read-only access
  guest:
    allow:
      - action: read
        if: "has_share_access(null, $this.id)"
      - action: download
        if: "has_share_access(null, $this.id)"
    deny:
      - action: create
      - action: update
      - action: delete
      - action: list
```

### Permission Expression Variables

| Variable | Description | Example |
|----------|-------------|---------|
| `$this` | Current entity | `$this.owner_id` |
| `$actor` | Current user | `$actor.id` |
| `$actor.has_role(role)` | Check user role | `$actor.has_role('admin')` |
| Custom functions | Module functions | `has_share_access($actor.id, $this.id)` |

---

## Attribute-Based Access Control (ABAC)

For requirements that go beyond role-based allow/deny lists (department-scoped access, clearance levels, time-of-day restrictions, IP filters), use the policy/ABAC system. Policies live under the `authorization:` block in the model file (not the hook file) — but they are evaluated in the same authorization pipeline as hook permissions and are documented here for completeness.

### Policies

```yaml
# Inside *.model.yaml under authorization:
authorization:
  policies:
    can_update_user:
      description: "Permission OR ownership"
      type: any                # any (OR) | all (AND)
      rules:
        - permission: users.update
        - owner:
            resource: User
            field: id
            actor_field: id

    can_delete_user:
      type: all
      rules:
        - permission: users.delete
        - not:
            condition: "resource.id == subject.id"
            message: "Cannot delete your own account"
        - condition: "resource.status != 'protected'"
          message: "Cannot delete protected accounts"
```

### Resource Policies

Bind policies to entity actions:

```yaml
authorization:
  resource_policies:
    User:
      read:
        - permission: users.read
      update:
        - policy: can_update_user
      delete:
        - policy: can_delete_user
```

### ABAC Attributes

Declare the attributes that policies can reference:

```yaml
authorization:
  attributes:
    subject:
      - id
      - role
      - department
      - clearance_level
      - mfa_verified
    resource:
      - owner_id
      - department
      - classification
      - status
    environment:
      - time
      - ip_address
      - device_type

  abac_policies:
    department_access:
      description: "Users only access resources in their department"
      condition: "subject.department == resource.department"
    clearance:
      condition: "subject.clearance_level >= resource.classification"
```

Permissions in `*.hook.yaml` are evaluated **first** (fast path); if no role-level rule applies, the policy engine evaluates the matching `resource_policies` entry.

---

## Triggers

### Trigger Types

| Trigger Name | When Fired |
|--------------|------------|
| `before_create` | Before entity is created |
| `after_create` | After entity is created |
| `before_update` | Before entity is updated |
| `after_update` | After entity is updated |
| `after_update_{field}` | After specific field changes |
| `before_delete` | Before entity is deleted |
| `after_delete` | After entity is deleted |

### Trigger Structure

```yaml
triggers:
  trigger_name:
    actions:
      - "action1"
      - "action2"
    if: "optional_condition"      # Only execute if true
```

### Trigger Examples

```yaml
triggers:
  # After create - log and emit event
  after_create:
    actions:
      - "create: FileAccessLog(file_id: id, user_id: owner_id, action: 'upload', accessed_at: now())"
      - "log(file.created, { file_id: id, owner_id: owner_id, size: size_bytes })"
      - "emit: FileCreatedEvent(file_id: id, owner_id: owner_id, bucket_id: bucket_id)"

  # After field update - specific field trigger
  after_update_status:
    actions:
      - "log(file.status_changed, { file_id: id, old_status: $old.status, new_status: status })"

  # After update with field access
  after_update_download_count:
    actions:
      - "set: last_accessed_at = now()"
      - "emit: FileDownloadedEvent(file_id: id, user_id: $context.user_id)"

  # Before delete - logging
  before_delete:
    actions:
      - "log(file.deleting, { file_id: id })"

  # After delete - cleanup
  after_delete:
    actions:
      - "delete: FileShare(file_id: id)"
      - "delete: FileAccessLog(file_id: id)"
      - "log(file.deleted, { file_id: id, owner_id: owner_id })"

  # Conditional trigger
  notify_high_priority:
    if: "priority == 'critical'"
    actions:
      - "notify([admin], Critical item created: #{name})"
      - "send_email(critical_alert, admin@example.com, { id: id, name: name })"
```

### Scheduled Triggers (in index.hook.yaml)

```yaml
# In index.hook.yaml
scheduled_jobs:
  cleanup_expired_files:
    schedule: "0 0 * * *"         # Daily at midnight
    actions:
      - "query: StoredFile(status == 'deleted' && deleted_at < now() - 30.days)"
      - "foreach: purge_file(file)"

  quota_report:
    schedule: "0 9 * * 1"         # Weekly Monday 9am
    actions:
      - "emit: WeeklyQuotaReportEvent()"
```

---

## Computed Fields

### Basic Computed Fields

```yaml
computed:
  # Simple expression
  is_image: "mime_type.starts_with('image/')"
  is_video: "mime_type.starts_with('video/')"

  # Calculation
  compression_ratio: "original_size != null ? (size_bytes / original_size) : 1.0"

  # Combined conditions
  is_safe: "is_scanned == true && (threat_level == null || threat_level == 'safe')"
  is_accessible: "status == 'active'"

  # String concatenation
  full_path: "bucket.root_path + '/' + path"

  # Date calculations
  age_days: "(now() - metadata.created_at).days()"

  # Multi-line expression
  display_status: |
    status == 'active' ? 'Available' :
    status == 'quarantined' ? 'Security Hold' :
    status == 'deleted' ? 'Trashed' :
    'Unknown'
```

### Computed Field Rules

1. **Read-only**: Computed fields cannot be set directly
2. **Not stored**: Calculated at runtime, not persisted
3. **Can reference**: Other fields, relations, functions
4. **Performance**: Keep expressions simple for frequently accessed fields

---

## Expression Syntax

### Field References

| Syntax | Description | Example |
|--------|-------------|---------|
| `field` | Direct field | `status`, `owner_id` |
| `$this.field` | Explicit current entity | `$this.owner_id` |
| `$old.field` | Previous value (in update) | `$old.status` |
| `$actor.field` | Current user | `$actor.id` |
| `$context.var` | Context variable | `$context.threat_name` |
| `relation.field` | Related entity field | `bucket.name` |

### Comparison Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `==` | Equal | `status == 'active'` |
| `!=` | Not equal | `status != 'deleted'` |
| `<` | Less than | `amount < 100` |
| `<=` | Less or equal | `amount <= 100` |
| `>` | Greater than | `amount > 0` |
| `>=` | Greater or equal | `age >= 18` |

### Logical Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `&&` | AND | `a == 1 && b == 2` |
| `\|\|` | OR | `a == 1 \|\| a == 2` |
| `!` | NOT | `!is_deleted` |

### String Methods

| Method | Description | Example |
|--------|-------------|---------|
| `.length` | String length | `name.length <= 100` |
| `.contains(str)` | Contains substring | `path.contains('..')` |
| `.starts_with(str)` | Starts with | `mime_type.starts_with('image/')` |
| `.ends_with(str)` | Ends with | `filename.ends_with('.pdf')` |
| `matches 'regex'` | Regex match | `email matches '^[a-z]+@'` |

### Null Handling

| Syntax | Description | Example |
|--------|-------------|---------|
| `== null` | Is null | `deleted_at == null` |
| `!= null` | Is not null | `owner_id != null` |
| `?? default` | Null coalesce | `name ?? 'Untitled'` |
| `?.field` | Safe navigation | `manager?.name` |

### Date/Time

| Expression | Description |
|------------|-------------|
| `now()` | Current timestamp |
| `today()` | Current date |
| `date - INTERVAL 'N days'` | Subtract days |
| `date + INTERVAL 'N hours'` | Add hours |
| `(date1 - date2).days()` | Difference in days |

### Collection Operations

| Method | Description | Example |
|--------|-------------|---------|
| `.length` | Array length | `items.length > 0` |
| `.contains(val)` | Contains value | `tags.contains('urgent')` |
| `.any(expr)` | Any matches | `items.any(i => i.status == 'pending')` |
| `.all(expr)` | All match | `items.all(i => i.valid)` |

### Aggregate Functions

| Function | Description | Example |
|----------|-------------|---------|
| `count(Entity, cond)` | Count entities | `count(Order, status == 'pending')` |
| `sum(Entity, field, cond)` | Sum field | `sum(OrderItem, amount, order_id == id)` |
| `exists(table, field, val)` | Exists check | `exists(users, id, owner_id)` |

### Role Checks

```yaml
# Check if actor has role
"$actor.has_role('admin')"

# Multiple role check
"$actor.has_role('admin') || $actor.has_role('manager')"

# Combined with ownership
"$actor.id == owner_id || $actor.has_role('admin')"
```

---

## Common Mistakes

### 1. Missing or Multiple Initial States

```yaml
# WRONG - No initial state
states:
  values:
    pending:
    active:

# WRONG - Multiple initial states
states:
  values:
    draft:
      initial: true
    pending:
      initial: true

# CORRECT - Exactly one initial
states:
  values:
    draft:
      initial: true
    pending:
    active:
```

### 2. Transition to Non-Existent State

```yaml
# WRONG - 'approved' not in values
states:
  values:
    pending:
      initial: true
    active:

  transitions:
    approve:
      from: pending
      to: approved           # State doesn't exist!

# CORRECT
states:
  values:
    pending:
      initial: true
    approved:                # Add the state
    active:

  transitions:
    approve:
      from: pending
      to: approved
```

### 3. Invalid From Array Syntax

```yaml
# WRONG - Missing brackets
transitions:
  soft_delete:
    from: active, suspended
    to: deleted

# CORRECT
transitions:
  soft_delete:
    from: [active, suspended]
    to: deleted
```

### 4. Rule Without Condition

```yaml
# WRONG - Missing condition
rules:
  my_rule:
    message: "Error message"

# CORRECT
rules:
  my_rule:
    condition: "field.length > 0"
    message: "Field cannot be empty"
```

### 5. Permission Without Role

```yaml
# WRONG - Not under a role
permissions:
  allow:
    - read

# CORRECT
permissions:
  user:                      # Role name required
    allow:
      - read
```

### 6. Trigger with Wrong Name Pattern

```yaml
# WRONG - Invalid trigger names
triggers:
  onCreate:                  # Should be snake_case
    actions: [...]

  update_before:             # Should be before_update
    actions: [...]

# CORRECT
triggers:
  after_create:
    actions: [...]

  before_update:
    actions: [...]
```

### 7. Action String Syntax Errors

```yaml
# WRONG - Missing quotes/format
on_enter:
  - emit OrderCreated         # Wrong syntax

# CORRECT
on_enter:
  - "emit: OrderCreatedEvent(order_id: id)"
```

### 8. Computed Field with Side Effects

```yaml
# WRONG - Computed can't modify
computed:
  process_total: |
    total = items.sum(amount)     # Can't assign!
    save()                        # Can't call mutations!
    total

# CORRECT - Pure expression only
computed:
  total: "items.sum(i => i.amount)"
```

---

## Quick Reference Checklist

### State Machine
- [ ] `states.field` matches an enum field in the model
- [ ] Exactly ONE state has `initial: true`
- [ ] All state names are `snake_case`
- [ ] All transitions reference existing states
- [ ] Transition `from` uses brackets for multiple: `[state1, state2]`
- [ ] Guard conditions are valid expressions

### Rules
- [ ] Each rule has a `condition` expression
- [ ] Each rule has a `message` for users
- [ ] `when` values are: `create`, `update`, or both
- [ ] Expressions reference valid fields

### Permissions
- [ ] Permissions are grouped by role name
- [ ] `allow` and `deny` contain valid actions
- [ ] Conditional permissions use valid expressions
- [ ] `only` field lists exist in the model

### Triggers
- [ ] Trigger names follow pattern: `before_*` or `after_*`
- [ ] Actions are properly quoted strings
- [ ] Conditional triggers have valid `if` expressions

### Computed Fields
- [ ] Expressions are pure (no side effects)
- [ ] Referenced fields exist in the model
- [ ] Multi-line expressions use `|` YAML syntax
