# Schema Examples

> **Purpose**: This document shows complete, real-world schema files from the production `bersihir` and `sapiens` modules. Every example below is **copied verbatim** (or only lightly trimmed) from `libs/modules/bersihir/schema/` and `libs/modules/sapiens/schema/`. They are kept in sync with the source so AI tools and developers can copy them as starting templates.
>
> If you need the raw files, browse:
> - [libs/modules/bersihir/schema/](../../libs/modules/bersihir/schema/)
> - [libs/modules/sapiens/schema/](../../libs/modules/sapiens/schema/)

## Table of Contents

1. [Example 1: Module Index File](#example-1-module-index-file)
2. [Example 2: Simple Entity Model](#example-2-simple-entity-model)
3. [Example 3: Entity Hook with Complex State Machine](#example-3-entity-hook-with-complex-state-machine)
4. [Example 4: Sub-Workflow (Phase 10 Pattern)](#example-4-sub-workflow-phase-10-pattern)
5. [Example 5: Cross-Module Foreign Key](#example-5-cross-module-foreign-key)
6. [Example 6: Per-Entity Generator Filtering](#example-6-per-entity-generator-filtering)

---

## Example 1: Module Index File

The `index.model.yaml` is required and defines the module, shared types, generator filtering, external imports, and the list of entity files. This example is from [libs/modules/bersihir/schema/models/index.model.yaml](../../libs/modules/bersihir/schema/models/index.model.yaml).

```yaml
module: bersihir
version: 2
description: "Bersihir Laundry Marketplace - Complete Schema Definitions"

# =============================================================================
# MODULE CONFIGURATION
# =============================================================================
config:
  database: postgresql
  soft_delete: true
  audit: true
  default_timestamps: true
  generators:
    # Blacklist mode — these targets are skipped for the entire module
    disabled:
      - graphql
      - grpc
      - proto
      - openapi

# =============================================================================
# EXTERNAL IMPORTS — Reuse sapiens for user management
# =============================================================================
external_imports:
  - module: sapiens
    types: [User, Profile, Session]

# =============================================================================
# SHARED TYPES — Available to all models in this module
# =============================================================================
shared_types:
  Timestamps:
    created_at:
      type: datetime
      attributes: ["@default(now)"]
      description: "Record creation timestamp"
    updated_at:
      type: datetime
      attributes: ["@updated_at"]
      description: "Last update timestamp"
    deleted_at:
      type: datetime?
      description: "Soft delete timestamp"

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

  # Composition: combines Timestamps + Actors into a single reusable type
  Metadata: [Timestamps, Actors]

  Money:
    amount:
      type: decimal
      attributes: ["@precision(18,2)", "@non_negative"]
      description: "Monetary amount"
    currency:
      type: string
      attributes: ["@default('IDR')", "@length(3)"]
      description: "ISO 4217 currency code"

  GeoLocation:
    latitude:
      type: float?
      attributes: ["@range(-90, 90)"]
    longitude:
      type: float?
      attributes: ["@range(-180, 180)"]

# =============================================================================
# IMPORT ALL MODEL FILES
# =============================================================================
imports:
  - location.model.yaml
  - address.model.yaml
  - customer.model.yaml
  - provider.model.yaml
  - order.model.yaml
  # ... 70+ more files
```

**Notes:**
- `module:` and `version:` are required.
- `config.generators.disabled` is the **blacklist mode** for generator filtering. The whitelist mode uses `enabled:` instead. See [GENERATION.md → Per-Module and Per-Model Filtering](./GENERATION.md#per-module-and-per-model-filtering).
- `Metadata: [Timestamps, Actors]` is composition syntax — it merges fields from two existing types.
- `external_imports` brings types from another module into this module's namespace. Use them as `sapiens.User` in foreign keys.
- `imports:` lists every entity file. Order does not matter.

---

## Example 2: Simple Entity Model

This is the complete `address.model.yaml` from Bersihir — a clean example showing every common feature: required and optional fields, foreign keys, custom enum, decimal precision, JSONB metadata via `@audit_metadata`, relations with inverse references, and indexes.

Source: [libs/modules/bersihir/schema/models/address.model.yaml](../../libs/modules/bersihir/schema/models/address.model.yaml)

```yaml
models:
  - name: Address
    collection: addresses
    description: "Reusable address entity, linked to customers/providers/outlets via join tables"

    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]

      # Address fields
      street_address:
        type: string
        attributes: ["@required", "@max(500)"]
      street_address_2:
        type: string?
        attributes: ["@max(255)"]
      postal_code:
        type: string?
        attributes: ["@max(10)"]

      # Geo location — separate @precision and @scale attributes
      latitude:
        type: decimal?
        attributes: ["@precision(10)", "@scale(8)"]
      longitude:
        type: decimal?
        attributes: ["@precision(11)", "@scale(8)"]

      # Address metadata
      label:
        type: string
        attributes: ["@required", "@max(50)"]
        description: "Address label (e.g., 'Rumah', 'Kantor')"
      address_type:
        type: AddressType
        attributes: ["@default(home)"]

      # Foreign keys to location hierarchy
      country_id:
        type: uuid?
        attributes: ["@foreign_key(Country.id)"]
      province_id:
        type: uuid?
        attributes: ["@foreign_key(Province.id)"]
      city_id:
        type: uuid?
        attributes: ["@foreign_key(City.id)"]
      district_id:
        type: uuid?
        attributes: ["@foreign_key(District.id)"]
      subdistrict_id:
        type: uuid?
        attributes: ["@foreign_key(Subdistrict.id)"]

      # Recipient info (may differ from user)
      recipient_name:
        type: string?
        attributes: ["@max(100)"]
      recipient_phone:
        type: phone?

      # Flags
      is_pickup_eligible:
        type: bool
        attributes: ["@default(true)"]
      is_delivery_eligible:
        type: bool
        attributes: ["@default(true)"]
      is_verified:
        type: bool
        attributes: ["@default(false)"]
      verified_at:
        type: datetime?

      # Audit metadata as a single JSONB column
      metadata:
        type: Metadata
        attributes: ["@audit_metadata"]

    relations:
      customer_addresses:
        type: CustomerAddress[]
        attributes: ["@one_to_many"]
        inverse: address

      country:
        type: Country?
        attributes: ["@one", "@foreign_key(country_id)"]
        inverse: addresses
      province:
        type: Province?
        attributes: ["@one", "@foreign_key(province_id)"]
        inverse: addresses
      city:
        type: City?
        attributes: ["@one", "@foreign_key(city_id)"]
        inverse: addresses
      district:
        type: District?
        attributes: ["@one", "@foreign_key(district_id)"]
        inverse: addresses
      subdistrict:
        type: Subdistrict?
        attributes: ["@one", "@foreign_key(subdistrict_id)"]
        inverse: addresses

    indexes:
      - type: index
        fields: [city_id]
      - type: index
        fields: [district_id]
      - type: index
        fields: [subdistrict_id]
      - type: index
        fields: [postal_code]

enums:
  - name: AddressType
    description: "Type of address"
    variants:
      - name: home
        description: "Home address"
        default: true
      - name: work
      - name: other
```

**Key takeaways:**
- The `id` field always uses `["@id", "@default(uuid)"]`.
- Optional fields use `?` as a type suffix (`string?`, `decimal?`, `datetime?`).
- Custom enums are declared in the same file and referenced by name (`type: AddressType`).
- Foreign keys use `@foreign_key(Model.field)` — the validator checks that the target model exists.
- The `metadata` field with `@audit_metadata` produces a single JSONB column instead of six separate audit columns.
- `inverse:` on relations gives the back-reference name in the other entity.

---

## Example 3: Entity Hook with Complex State Machine

This is an excerpt from `order.hook.yaml` — Bersihir's order lifecycle is the most complex state machine in the system, with 18 states and ~25 transitions. The full file is at [libs/modules/bersihir/schema/hooks/order.hook.yaml](../../libs/modules/bersihir/schema/hooks/order.hook.yaml).

```yaml
model: Order

states:
  field: status
  values:
    - name: draft
      initial: true
      description: "Order being created"
      on_enter:
        - action: generate_order_number

    - name: pending_payment
      description: "Awaiting payment"
      on_enter:
        - action: calculate_total
        - action: apply_promotions
        - action: create_payment_record
        - action: set_payment_deadline

    - name: payment_confirmed
      description: "Payment received"
      on_enter:
        - action: confirm_payment
        - action: notify_provider
        - action: award_loyalty_points

    - name: pending_pickup
      on_enter:
        - action: create_pickup_task

    - name: pickup_scheduled
      on_enter:
        - action: assign_pickup_agent
        - action: notify_customer_pickup_scheduled

    # ... 12 more intermediate states ...

    - name: completed
      final: true
      on_enter:
        - action: finalize_order
        - action: calculate_provider_commission
        - action: send_completion_notification

    - name: cancelled
      final: true
      on_enter:
        - action: process_cancellation
        - action: handle_refund_if_paid

  transitions:
    - from: draft
      to: pending_payment
      event: submit
      description: "Customer submits order"
      roles: [customer, staff, system]
      condition: "items.length > 0"
      actions:
        - action: validate_order_items
        - action: check_service_availability

    - from: pending_payment
      to: payment_confirmed
      event: confirm_payment
      roles: [system, staff, manager]
      condition: "payment.status == 'paid'"

    - from: pending_payment
      to: cancelled
      event: payment_expired
      roles: [system]
      condition: "NOW() > payment_deadline"

    - from: payment_confirmed
      to: pending_pickup
      event: request_pickup
      roles: [customer, staff, system]
      condition: "delivery_type == 'pickup_delivery'"

    # ... 20+ more transitions ...
```

**Key takeaways:**
- `model: Order` references the entity by name. The hook file does not need to repeat the entity's fields.
- `field: status` tells the state machine which enum field tracks state.
- Exactly **one** state has `initial: true`. Multiple states can have `final: true`.
- `on_enter` runs business actions when the entity transitions into that state.
- Transitions can be guarded by `roles:` (RBAC) and `condition:` (business rule).
- Each transition can run additional actions before commit.
- The Phase 8 `StateMachineBehavior` trait enforces these transitions at the entity level — illegal transitions become impossible to write.

For the rules, permissions, triggers, and computed sections, see [RULE_FORMAT_HOOKS.md](./RULE_FORMAT_HOOKS.md).

---

## Example 4: Sub-Workflow (Phase 10 Pattern)

This is the recommended workflow shape after Phase 10. Instead of one monolithic `OrderProcessing.workflow.yaml` with 60+ steps, the order pipeline is decomposed into three sub-workflows that chain via domain events:

```
OrderCreatedEvent
  → [OrderValidation] emits OrderPaymentConfirmedEvent
  → [OrderFulfillment] emits OrderReadyForDeliveryEvent
  → [OrderDelivery] terminates
```

Source: [libs/modules/bersihir/schema/workflows/order_validation.workflow.yaml](../../libs/modules/bersihir/schema/workflows/order_validation.workflow.yaml)

```yaml
name: OrderValidation
description: |
  Order validation and payment workflow.
  Covers order initialization, validation, total calculation,
  payment record creation, and payment confirmation.
  Chains to OrderFulfillment via OrderPaymentConfirmedEvent.

version: 1

trigger:
  event: OrderCreatedEvent
  extract:
    order_id: "event.order_id"
    customer_id: "event.customer_id"
    provider_id: "event.provider_id"
    outlet_id: "event.outlet_id"
    total_amount: "event.total_amount"

config:
  timeout: 26h            # 24h payment window + 2h buffer
  persistence: true
  retry:
    max_attempts: 3
    backoff: exponential

context:
  order: null
  payment: null
  sla_deadline: null

steps:
  - name: load_order
    type: action
    action: query
    entity: Order
    params:
      id: "{{ context.order_id }}"
    on_success:
      set:
        order: "{{ result }}"
      next: validate_order
    on_failure:
      next: order_not_found

  - name: validate_order
    type: action
    action: validate
    params:
      rules:
        - "order.items.length > 0"
        - "order.total_amount > 0"
        - "order.outlet.status == 'active'"
        - "order.customer.status != 'blacklisted'"
    on_success:
      next: calculate_totals
    on_failure:
      next: validation_failed

  - name: create_payment_record
    type: action
    action: create
    entity: Payment
    params:
      order_id: "{{ context.order_id }}"
      amount: "{{ context.order.total_amount }}"
      status: "pending"
      payment_deadline: "{{ now() + 24h }}"
    on_success:
      set:
        payment: "{{ result }}"
      next: transition_to_pending_payment

  - name: transition_to_pending_payment
    type: transition
    entity: Order
    id: "{{ context.order_id }}"
    transition: submit
    on_success:
      next: notify_customer_payment

  - name: wait_for_payment
    type: wait
    wait_for:
      event: PaymentReceivedEvent
      condition: "event.order_id == context.order_id"
      timeout: 24h
    on_event:
      set:
        payment: "{{ event }}"
      next: confirm_payment
    on_timeout:
      next: payment_expired

  - name: confirm_payment
    type: transition
    entity: Order
    id: "{{ context.order_id }}"
    transition: confirm_payment
    on_success:
      next: emit_payment_confirmed_event

  - name: emit_payment_confirmed_event
    type: action
    action: emit_event
    params:
      event: OrderPaymentConfirmedEvent      # ← chains to next sub-workflow
      data:
        order_id: "{{ context.order_id }}"
        customer_id: "{{ context.customer_id }}"
    on_success:
      next: complete

  - name: complete
    type: terminal
    status: completed
    result:
      order_id: "{{ context.order_id }}"
      payment_id: "{{ context.payment.id }}"

  # --- Error terminals ---
  - name: order_not_found
    type: terminal
    status: failed
    reason: "Order not found"

  - name: validation_failed
    type: terminal
    status: failed
    reason: "Order validation failed"
```

**Key takeaways:**
- The workflow is **triggered by an event**, not invoked directly.
- `extract:` maps event fields into the workflow's `context`.
- Every non-terminal step has `on_success.next` (and usually `on_failure.next`).
- The workflow ends by emitting **another event** (`OrderPaymentConfirmedEvent`) which triggers the next sub-workflow in the chain.
- Each sub-workflow is small enough to reason about and test in isolation.

See [RULE_FORMAT_WORKFLOWS.md](./RULE_FORMAT_WORKFLOWS.md) for the complete syntax reference.

---

## Example 5: Cross-Module Foreign Key

When a Bersihir entity references a Sapiens user, the foreign key uses the `module.Type` syntax. This requires `external_imports` in `index.model.yaml`.

```yaml
# bersihir/schema/models/index.model.yaml
external_imports:
  - module: sapiens
    types: [User, Profile, Session]
```

```yaml
# bersihir/schema/models/customer.model.yaml
models:
  - name: Customer
    collection: customers
    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]

      # Foreign key to User in another module
      user_id:
        type: uuid
        attributes: ["@required", "@foreign_key(sapiens.User.id)"]

      loyalty_tier:
        type: string
        attributes: ["@default('bronze')"]

      metadata:
        type: Metadata
        attributes: ["@audit_metadata"]

    relations:
      user:
        type: sapiens.User?
        attributes: ["@one", "@foreign_key(user_id)"]
```

The validator checks that `sapiens` is in `external_imports` and that `User` is in the imported types list. The generator emits a real PostgreSQL FK constraint and a typed `UserId` reference in the Rust entity.

---

## Example 6: Per-Entity Generator Filtering

Internal entities (audit logs, internal queues, denormalised caches) often shouldn't be exposed via HTTP, gRPC, GraphQL, or OpenAPI. Use per-entity `generators.disabled` to skip those targets for that entity only.

```yaml
# audit_log.model.yaml — internal entity, no public API
models:
  - name: AuditLog
    collection: audit_logs
    fields:
      id:
        type: uuid
        attributes: ["@id", "@default(uuid)"]
      actor_id:
        type: uuid
        attributes: ["@required"]
      action:
        type: string
        attributes: ["@required", "@max(100)"]
      target_type:
        type: string
        attributes: ["@required", "@max(100)"]
      target_id:
        type: uuid
        attributes: ["@required"]
      payload:
        type: json?
      created_at:
        type: datetime
        attributes: ["@default(now)"]

    indexes:
      - type: index
        fields: [actor_id, created_at]
      - type: index
        fields: [target_type, target_id]

    generators:
      disabled: [handler, grpc, graphql, openapi, dto]
```

The generator still emits `sql`, `rust`, `repository`, `service`, etc. — but no public API surface. Per-entity filtering takes precedence over the module-level config.

---

## Where to Go Next

| Topic | Document |
|-------|----------|
| Field types, attributes, relations, indexes | [RULE_FORMAT_MODELS.md](./RULE_FORMAT_MODELS.md) |
| State machines, rules, permissions, triggers | [RULE_FORMAT_HOOKS.md](./RULE_FORMAT_HOOKS.md) |
| Workflow steps, conditions, loops, compensation | [RULE_FORMAT_WORKFLOWS.md](./RULE_FORMAT_WORKFLOWS.md) |
| Generator targets, CLI flags, custom code | [GENERATION.md](./GENERATION.md) |
| Layer overview and generated file layout | [ARCHITECTURE.md](./ARCHITECTURE.md) |
| Cross-module integration patterns | [INTEGRATION.md](./INTEGRATION.md) |
| Type system, shared types, value objects | [TYPES.md](./TYPES.md) |
