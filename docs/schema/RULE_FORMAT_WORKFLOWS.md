# Workflow Schema YAML Rules & Format

> **CRITICAL**: Follow these rules exactly to prevent validation errors. Workflow validation is strict - every step must be reachable and properly connected.

This document is the **canonical authoring reference** for `*.workflow.yaml` files. It is the merger of the previous WORKFLOW.md and RULE_FORMAT_WORKFLOWS.md.

> **Generator note (Phases 5 + 10)**: Workflows and triggers compose via `backbone-core` generic types — there are no per-entity workflow adapters. Prefer **focused sub-workflows** over monolithic ones. Decompose long workflows into stages chained via domain events: stage 1 emits a `StageCompletedEvent`, stage 2 is triggered by that event, and so on. The Bersihir order pipeline is the reference implementation: `OrderCreatedEvent → [OrderValidation] → OrderPaymentConfirmedEvent → [OrderFulfillment] → OrderReadyForDeliveryEvent → [OrderDelivery] → complete`. Each sub-workflow is small enough to reason about and test in isolation.

## Table of Contents

1. [Overview](#overview)
2. [File Organization](#file-organization)
3. [Workflow Structure](#workflow-structure)
4. [Triggers](#triggers)
5. [Configuration](#configuration)
6. [Context Variables](#context-variables)
7. [Step Types](#step-types)
8. [Step Transitions](#step-transitions)
9. [Loop Steps](#loop-steps)
10. [Parallel Steps](#parallel-steps)
11. [Condition Steps](#condition-steps)
12. [Terminal Steps](#terminal-steps)
13. [Compensation (Rollback)](#compensation-rollback)
14. [Expression Syntax](#expression-syntax)
15. [Common Mistakes](#common-mistakes)
16. [Validation Rules](#validation-rules)

---

## Overview

Workflows define **multi-step business processes** (Saga pattern):
- **Trigger**: How the workflow starts (event, schedule, endpoint)
- **Steps**: Sequence of actions, conditions, loops, waits
- **Compensation**: Rollback actions if workflow fails

---

## File Organization

```
libs/modules/{module}/schema/workflows/
├── index.workflow.yaml          # Optional: Module workflow config
├── {workflow_name}.workflow.yaml # Workflow definitions
└── ...
```

### Naming Conventions

| Element | Convention | Example |
|---------|------------|---------|
| File names | `snake_case.workflow.yaml` | `file_upload.workflow.yaml` |
| Workflow names | `PascalCase` | `FileUpload`, `OrderProcessing` |
| Step names | `snake_case` | `load_data`, `check_quota` |
| Context variables | `snake_case` | `bucket`, `scan_result` |

---

## Workflow Structure

### Complete Template

```yaml
# Workflow metadata
name: WorkflowName                # Required: PascalCase
description: |                    # Optional: Multi-line description
  Workflow description here.

version: 1                        # Required: Schema version

# How workflow is triggered
trigger:
  event: EventName                # OR schedule OR endpoint
  extract:
    var_name: "event.field_path"

# Workflow configuration
config:
  timeout: 30m                    # Max duration
  persistence: true               # Store workflow state
  transaction_mode: saga          # atomic | saga | hybrid
  retry:
    max_attempts: 3
    backoff: exponential

# Workflow variables
context:
  var_name: null
  other_var: default_value

# Workflow steps
steps:
  - name: first_step
    type: action
    # ... step definition

  - name: terminal_step
    type: terminal
    status: completed

# Optional: Rollback handlers
compensation:
  - name: undo_action
    condition: "steps.action_step.completed == true"
    action: rollback_action
```

---

## Triggers

### Event Trigger

```yaml
trigger:
  event: FileUploadRequestedEvent
  extract:
    bucket_id: "event.bucket_id"
    path: "event.path"
    owner_id: "event.owner_id"
```

### Schedule Trigger (Cron)

```yaml
trigger:
  schedule: "*/15 * * * *"        # Every 15 minutes
  # OR
  schedule: "0 0 * * *"           # Daily at midnight
  # OR
  schedule: "0 9 * * 1"           # Weekly Monday 9am
```

### HTTP Endpoint Trigger

```yaml
trigger:
  endpoint: /api/v1/workflows/process
  method: POST
  extract:
    data: "request.body"
    user_id: "request.user.id"
```

### Extract Variables

```yaml
trigger:
  event: OrderCreatedEvent
  extract:
    # Simple field extraction
    order_id: "event.order_id"
    customer_id: "event.customer_id"

    # Nested field extraction
    total: "event.order.total_amount"

    # With default
    priority: "event.priority ?? 'normal'"
```

---

## Configuration

### Complete Config Options

```yaml
config:
  # Timeout for entire workflow
  timeout: 30m                    # 30 minutes
  # OR
  timeout: 24h                    # 24 hours
  # OR
  timeout: 5m                     # 5 minutes

  # Persist workflow state (for long-running)
  persistence: true               # Default: false

  # Transaction mode
  transaction_mode: saga          # saga | atomic | hybrid

  # What happens on timeout
  on_timeout: cancel              # cancel | compensate | continue

  # Retry configuration
  retry:
    max_attempts: 3               # How many retries
    backoff:
      type: exponential           # exponential | linear | fixed
      initial: 1s                 # Initial delay
      max: 5m                     # Max delay
      multiplier: 2               # For exponential
```

### Timeout Formats

| Format | Example | Duration |
|--------|---------|----------|
| Seconds | `30s` | 30 seconds |
| Minutes | `5m` | 5 minutes |
| Hours | `2h` | 2 hours |
| Days | `1d` | 1 day |
| Combined | `1h30m` | 1 hour 30 minutes |

---

## Context Variables

### Declaring Context

```yaml
context:
  # Null initial value
  bucket: null
  file: null
  scan_result: null

  # With default value
  retry_count: 0
  max_retries: 3

  # Array initial value
  items_to_process: []

  # Object initial value
  stats:
    processed: 0
    failed: 0
```

### Using Context in Steps

```yaml
steps:
  - name: use_context
    type: action
    action: query
    params:
      id: "{{ context.bucket_id }}"       # Reference context var

  - name: set_context
    type: action
    action: process
    on_success:
      set:
        result: "{{ result }}"            # Set context from result
        processed: "{{ context.processed + 1 }}"
      next: next_step
```

---

## Step Types

### Overview

| Type | Purpose | Key Fields |
|------|---------|------------|
| `action` | Execute an action | `action`, `params`, `entity` |
| `condition` | Branch based on condition | `conditions` |
| `loop` | Iterate over collection | `foreach`, `as`, `steps` |
| `parallel` | Execute branches in parallel | `branches`, `join` |
| `wait` | Wait for event/timeout | `wait_for`, `on_event` |
| `subprocess` | Call another workflow | `flow`, `params` |
| `human_task` | Wait for human input | `task`, `on_complete` |
| `transition` | Trigger state transition | `entity`, `transition` |
| `terminal` | End workflow | `status`, `result` |

---

## Step Transitions

> **CRITICAL**: Every step (except terminal) MUST have a `next` transition. Steps without transitions are considered "unreachable" and will fail validation.

### Basic Transition

```yaml
steps:
  - name: step_one
    type: action
    action: do_something
    on_success:
      next: step_two              # REQUIRED: Where to go next
    on_failure:
      next: error_handler         # What to do on failure

  - name: step_two
    type: terminal
    status: completed
```

### Setting Context on Transition

```yaml
steps:
  - name: load_data
    type: action
    action: query
    entity: User
    params:
      id: "{{ context.user_id }}"
    on_success:
      set:                        # Set context variables
        user: "{{ result }}"
        loaded_at: "{{ now() }}"
      next: process_user
    on_failure:
      next: user_not_found
```

### Retry Configuration

```yaml
steps:
  - name: external_api_call
    type: action
    action: call_api
    params:
      url: "{{ context.api_url }}"
    timeout: 30s                  # Step-level timeout
    on_success:
      next: process_response
    on_failure:
      retry: 3                    # Retry up to 3 times
      backoff: exponential
      next: api_failed            # After all retries exhausted
```

---

## Loop Steps

> **CRITICAL LESSON LEARNED**: The first step inside a loop is automatically reachable when the loop iterates. You do NOT need to add a transition to the first inner step.

### Basic Loop Structure

```yaml
steps:
  - name: process_items
    type: loop
    foreach: "{{ context.items }}"    # Collection to iterate
    as: item                          # Loop variable name
    on_success:                       # When loop completes
      next: all_done
    on_failure:                       # If any iteration fails
      next: handle_errors

    steps:                            # Inner steps
      - name: process_single_item     # First step - auto reachable!
        type: action
        action: process
        params:
          data: "{{ item }}"
        on_success:
          next: log_item              # Continue to next inner step
        on_failure:
          next: item_failed

      - name: log_item
        type: action
        action: log
        params:
          message: "Processed {{ item.id }}"
        on_success:
          next: item_done             # Go to terminal

      - name: item_failed
        type: action
        action: log_error
        params:
          error: "{{ error }}"
        on_success:
          next: item_done             # Still complete iteration

      - name: item_done
        type: terminal                # Terminal for this iteration
        status: success               # Loop continues to next item
```

### Nested Loops

```yaml
steps:
  - name: process_orders
    type: loop
    foreach: "{{ context.orders }}"
    as: order
    on_success:
      next: complete
    steps:
      - name: process_order_items     # First step of outer loop
        type: loop
        foreach: "{{ order.items }}"
        as: item
        on_success:
          next: order_complete
        steps:
          - name: process_item        # First step of inner loop - auto reachable!
            type: action
            action: process
            params:
              order_id: "{{ order.id }}"
              item_id: "{{ item.id }}"
            on_success:
              next: item_complete

          - name: item_complete
            type: terminal
            status: success

      - name: order_complete
        type: terminal
        status: success
```

### Loop with Index

```yaml
steps:
  - name: process_with_index
    type: loop
    foreach: "{{ context.items }}"
    as: item
    index_var: idx                    # Optional: index variable
    on_success:
      next: complete
    steps:
      - name: process
        type: action
        action: process
        params:
          item: "{{ item }}"
          position: "{{ idx }}"       # Access index (0-based)
        on_success:
          next: done

      - name: done
        type: terminal
        status: success
```

---

## Parallel Steps

> **CRITICAL**: Each parallel branch's first step is automatically reachable.

### Basic Parallel Structure

```yaml
steps:
  - name: parallel_processing
    type: parallel
    branches:
      - name: branch_a
        steps:
          - name: task_a1             # First step - auto reachable!
            type: action
            action: task_a
            on_success:
              next: branch_a_done

          - name: branch_a_done
            type: terminal
            status: success

      - name: branch_b
        steps:
          - name: task_b1             # First step - auto reachable!
            type: action
            action: task_b
            on_success:
              next: branch_b_done

          - name: branch_b_done
            type: terminal
            status: success

    join: all                         # Wait for all branches
    on_complete:
      next: after_parallel
```

### Join Strategies

```yaml
# Wait for ALL branches
join: all

# Wait for ANY branch (first to complete)
join: any

# Wait for N of M branches
join: n_of_m(2, 3)                    # 2 of 3 must complete
```

---

## Condition Steps

### Basic Condition

```yaml
steps:
  - name: check_status
    type: condition
    conditions:
      - if: "context.status == 'approved'"
        next: process_approved
      - if: "context.status == 'rejected'"
        next: process_rejected
      - else: true                    # Default case
        next: process_pending
```

### Multiple Conditions

```yaml
steps:
  - name: route_by_type
    type: condition
    conditions:
      - if: "context.amount > 10000"
        next: large_order_flow
      - if: "context.priority == 'urgent'"
        next: urgent_flow
      - if: "context.customer.tier == 'vip'"
        next: vip_flow
      - else: true                    # ALWAYS include else
        next: standard_flow
```

### Complex Conditions

```yaml
steps:
  - name: complex_check
    type: condition
    conditions:
      - if: "context.bucket != null && context.bucket.status == 'active'"
        next: proceed
      - if: "context.scan_result.is_safe == true"
        next: mark_safe
      - if: "context.scan_result.is_safe == false"
        next: quarantine
      - else: true
        next: pending_review
```

> **RULE**: Always include an `else` case to ensure all paths are covered.

---

## Terminal Steps

### Success Terminal

```yaml
steps:
  - name: complete
    type: terminal
    status: completed                 # or: success
    result:                           # Optional: Return data
      file: "{{ context.file }}"
      message: "Upload successful"
```

### Failed Terminal

```yaml
steps:
  - name: upload_failed
    type: terminal
    status: failed
    reason: "File upload failed"      # Error message
```

### Terminal with Event Emission

```yaml
steps:
  - name: complete_with_event
    type: terminal
    status: completed
    emit:
      event: WorkflowCompletedEvent
      data:
        workflow_id: "{{ context.workflow_id }}"
        result: "{{ context.result }}"
```

---

## Action Steps

### Query Action

```yaml
steps:
  - name: load_user
    type: action
    action: query
    entity: User
    params:
      id: "{{ context.user_id }}"
      # OR for list query:
      # where: "status == 'active'"
      # limit: 100
      # order_by: "created_at DESC"
    on_success:
      set:
        user: "{{ result }}"
      next: process_user
    on_failure:
      next: user_not_found
```

### Create Action

```yaml
steps:
  - name: create_record
    type: action
    action: create
    entity: StoredFile
    params:
      bucket_id: "{{ context.bucket_id }}"
      owner_id: "{{ context.owner_id }}"
      path: "{{ context.path }}"
      status: uploading
    on_success:
      set:
        file: "{{ result }}"
      next: store_content
```

### Update Action

```yaml
steps:
  - name: update_status
    type: action
    action: update
    entity: StoredFile
    params:
      id: "{{ context.file.id }}"
      status: active
      is_scanned: true
    on_success:
      next: complete
```

### Delete Action

```yaml
steps:
  - name: cleanup
    type: action
    action: delete
    entity: TempFile
    params:
      id: "{{ context.temp_file.id }}"
    on_success:
      next: complete
```

### Custom Action

```yaml
steps:
  - name: scan_file
    type: action
    action: scan_for_threats         # Custom action name
    params:
      content: "{{ context.content }}"
      filename: "{{ context.filename }}"
    timeout: 30s
    on_success:
      set:
        scan_result: "{{ result }}"
      next: check_scan
    on_timeout:                      # Handle timeout
      set:
        scan_result: "{ is_safe: null, pending: true }"
      next: mark_pending
    on_failure:
      next: scan_failed
```

### Emit Event Action

```yaml
steps:
  - name: emit_event
    type: action
    action: emit_event
    params:
      event: FileUploadedEvent
      data:
        file_id: "{{ context.file.id }}"
        owner_id: "{{ context.owner_id }}"
    on_success:
      next: complete
```

---

## Wait Steps

### Wait for Event

```yaml
steps:
  - name: wait_for_approval
    type: wait
    wait_for:
      event: ApprovalReceivedEvent
      condition: "event.order_id == context.order_id"
      timeout: 24h
    on_event:
      set:
        approval: "{{ event }}"
      next: process_approval
    on_timeout:
      next: approval_timeout
```

### Wait for Duration

```yaml
steps:
  - name: delay
    type: wait
    wait_for:
      duration: 5m                   # Wait 5 minutes
    on_event:
      next: continue_processing
```

---

## Human Task Steps

```yaml
steps:
  - name: manager_approval
    type: human_task
    task:
      title: "Approve Order #{{ context.order.id }}"
      description: "Please review and approve this order"
      assignee: "{{ context.order.manager_id }}"
      # OR assign by role:
      assignee_role: manager
      department: sales

      form:
        fields:
          - name: approved
            type: boolean
            label: "Approve this order?"
            required: true
          - name: comments
            type: text
            label: "Comments"
            required: false

      timeout: 48h

    on_complete:
      - if: "task.approved == true"
        next: order_approved
      - else: true
        next: order_rejected

    on_timeout:
      next: escalate_to_director
```

---

## Transition Steps

```yaml
steps:
  - name: approve_order
    type: transition
    entity: Order
    id: "{{ context.order.id }}"
    transition: approve              # State machine transition
    on_success:
      next: notify_customer
    on_failure:
      next: transition_failed
```

---

## Subprocess Steps

```yaml
steps:
  - name: process_payment
    type: subprocess
    flow: PaymentProcessing          # Another workflow name
    params:
      order_id: "{{ context.order.id }}"
      amount: "{{ context.order.total }}"
    wait: true                       # Wait for completion
    on_success:
      set:
        payment_result: "{{ result }}"
      next: check_payment
    on_failure:
      next: payment_failed
```

---

## Sub-Workflow Composition (Recommended Pattern)

For any workflow longer than ~10 steps, decompose it into a chain of focused sub-workflows that communicate via domain events. This pattern was introduced in Phase 10 of the Bersihir refactor.

### Example: Order Processing Chain

Instead of one `OrderProcessing.workflow.yaml` with 60+ steps:

```yaml
# 1. order_validation.workflow.yaml
name: OrderValidation
trigger:
  event: OrderCreatedEvent
steps:
  # ... validation, total calculation, payment record creation ...
  - name: emit_payment_confirmed
    type: action
    action: emit_event
    params:
      event: OrderPaymentConfirmedEvent
      data:
        order_id: "{{ context.order_id }}"
    on_success:
      next: complete
  - name: complete
    type: terminal
    status: completed
```

```yaml
# 2. order_fulfillment.workflow.yaml
name: OrderFulfillment
trigger:
  event: OrderPaymentConfirmedEvent     # ← chained from previous workflow
steps:
  # ... pickup, processing, QC ...
  - name: emit_ready_for_delivery
    type: action
    action: emit_event
    params:
      event: OrderReadyForDeliveryEvent
      data:
        order_id: "{{ context.order_id }}"
    on_success:
      next: complete
```

```yaml
# 3. order_delivery.workflow.yaml
name: OrderDelivery
trigger:
  event: OrderReadyForDeliveryEvent     # ← chained again
steps:
  # ... delivery, completion, rating ...
```

### Why Decompose

| Monolithic Workflow | Sub-Workflow Chain |
|--------------------|-------------------|
| Single 60-step file | Three focused files |
| Hard to reason about state | Each stage has clear entry/exit |
| One failure rolls back everything | Each stage compensates independently |
| Hard to test | Each stage testable in isolation |
| All-or-nothing deploys | Stages can deploy independently |

### Rules for Sub-Workflow Chains

1. **Chain via events, not direct calls** — emit a domain event, let the next workflow's trigger pick it up.
2. **Each sub-workflow owns its compensation** — don't try to compensate across stage boundaries.
3. **Name events after the state transition** — `OrderPaymentConfirmedEvent`, not `OrderValidationDoneEvent`.
4. **Document the chain** — list the event flow at the top of each sub-workflow file.
5. **For embedded sub-workflows** (synchronous call from one workflow into another), use the `subprocess` step type with `wait: true`.

---

## Compensation (Rollback)

### Compensation Structure

```yaml
compensation:
  - name: undo_action_name
    condition: "steps.action_step.completed == true"
    action: rollback_action
    entity: EntityName               # Optional
    params:
      id: "{{ context.entity.id }}"
```

### Complete Example

```yaml
compensation:
  # Delete stored file content
  - name: delete_stored_content
    condition: "steps.store_content.completed == true"
    action: delete_file
    params:
      key: "{{ context.file.storage_key }}"
      backend: "{{ context.bucket.storage_backend }}"

  # Delete database record
  - name: delete_file_record
    condition: "steps.create_file_record.completed == true"
    action: delete
    entity: StoredFile
    params:
      id: "{{ context.file.id }}"

  # Restore quota
  - name: restore_quota
    condition: "steps.update_quota.completed == true"
    action: update
    entity: UserQuota
    params:
      id: "{{ context.quota.id }}"
      used_bytes: "{{ context.quota.used_bytes - context.file.size_bytes }}"
```

---

## Expression Syntax

### Template Syntax

```yaml
# Context variable
"{{ context.variable }}"

# Result from previous step
"{{ result }}"

# Field from result
"{{ result.field_name }}"

# Nested access
"{{ context.user.profile.name }}"

# With default
"{{ context.value ?? 'default' }}"

# Calculation
"{{ context.count + 1 }}"

# String concatenation
"{{ context.base_path + '/' + context.filename }}"

# Null check
"{{ context.value ?? null }}"
```

### Condition Expressions

```yaml
# Equality
"context.status == 'active'"

# Null check
"context.bucket != null"

# Combined
"context.bucket != null && context.bucket.status == 'active'"

# Method call
"context.mime_type.starts_with('image/')"

# Array length
"context.items.length > 0"

# Contains
"context.tags.contains('urgent')"
```

---

## Common Mistakes

### 1. Unreachable Steps (CRITICAL)

> **This is the #1 cause of validation failures**

```yaml
# WRONG - No step transitions to 'process_item'
steps:
  - name: process_loop
    type: loop
    foreach: items
    steps:
      - name: process_item          # ERROR: Unreachable!
        type: action
        action: process
        on_success:
          next: done                 # Jumps out of loop

# CORRECT - First step in loop is auto-reachable
steps:
  - name: process_loop
    type: loop
    foreach: items
    as: item
    on_success:
      next: complete                # What to do after loop
    steps:
      - name: process_item          # OK: First step is reachable
        type: action
        action: process
        params:
          data: "{{ item }}"
        on_success:
          next: item_done           # Stay within loop

      - name: item_done
        type: terminal              # End of iteration
        status: success
```

### 2. Missing Terminal Step

```yaml
# WRONG - No terminal step
steps:
  - name: process
    type: action
    on_success:
      next: log
  - name: log
    type: action
    # No next, no terminal!

# CORRECT
steps:
  - name: process
    type: action
    on_success:
      next: log
  - name: log
    type: action
    on_success:
      next: complete
  - name: complete
    type: terminal
    status: completed
```

### 3. Condition Without Else

```yaml
# WRONG - What if neither condition matches?
steps:
  - name: check
    type: condition
    conditions:
      - if: "context.type == 'a'"
        next: process_a
      - if: "context.type == 'b'"
        next: process_b

# CORRECT
steps:
  - name: check
    type: condition
    conditions:
      - if: "context.type == 'a'"
        next: process_a
      - if: "context.type == 'b'"
        next: process_b
      - else: true                   # ALWAYS include!
        next: process_default
```

### 4. Loop Without Terminal for Iterations

```yaml
# WRONG - Inner steps don't complete properly
steps:
  - name: process_items
    type: loop
    foreach: items
    as: item
    on_success:
      next: done
    steps:
      - name: process
        type: action
        on_success:
          next: done               # Wrong: 'done' is outside loop!

# CORRECT - Use terminal within loop
steps:
  - name: process_items
    type: loop
    foreach: items
    as: item
    on_success:
      next: done
    steps:
      - name: process
        type: action
        on_success:
          next: item_complete

      - name: item_complete
        type: terminal             # Completes this iteration
        status: success

  - name: done
    type: terminal
    status: completed
```

### 5. Missing on_failure Handler

```yaml
# RISKY - What if action fails?
steps:
  - name: api_call
    type: action
    action: call_external_api
    on_success:
      next: process_response
    # No on_failure!

# CORRECT
steps:
  - name: api_call
    type: action
    action: call_external_api
    on_success:
      next: process_response
    on_failure:
      retry: 3                     # Retry first
      next: api_failed             # Then handle failure

  - name: api_failed
    type: terminal
    status: failed
    reason: "External API call failed"
```

### 6. Referencing Wrong Context

```yaml
# WRONG - 'result' is only available in on_success
steps:
  - name: load
    type: action
    action: query
    params:
      filter: "{{ result.id }}"    # ERROR: result not available yet!

# CORRECT - Use context
steps:
  - name: load
    type: action
    action: query
    on_success:
      set:
        data: "{{ result }}"       # Store in context
      next: use_data

  - name: use_data
    type: action
    params:
      filter: "{{ context.data.id }}"   # Access from context
```

### 7. Step Name Conflicts

```yaml
# WRONG - Duplicate names
steps:
  - name: process
    type: action
    on_success:
      next: complete

  - name: process                  # Duplicate!
    type: action

# CORRECT - Unique names
steps:
  - name: process_first
    type: action
    on_success:
      next: process_second

  - name: process_second
    type: action
```

---

## Validation Rules

The schema validator checks:

1. **Reachability**: Every step must be reachable from another step
   - First step of workflow is always reachable (entry point)
   - First step of loop/parallel branch is automatically reachable
   - All other steps must have another step transition to them

2. **Terminal Steps**: Workflow must have at least one terminal step

3. **Valid References**: All `next` values must reference existing step names

4. **Context Variables**: Variables should be declared in `context` before use

5. **Entity References**: Entity names in actions must exist in models

6. **Event References**: Events in triggers/emit must be defined

---

## Quick Reference Checklist

### Workflow Structure
- [ ] Has `name`, `version`, `trigger`, `steps`
- [ ] Name is `PascalCase`
- [ ] Has at least one `terminal` step

### Steps
- [ ] Every step has unique `name`
- [ ] Every non-terminal step has `on_success.next`
- [ ] Critical steps have `on_failure` handlers
- [ ] Step names are `snake_case`

### Loops
- [ ] Loop has `foreach`, `as`, `steps`
- [ ] Loop has `on_success.next` for after loop completes
- [ ] Inner steps have terminal for iteration completion
- [ ] Inner step transitions stay within the loop

### Conditions
- [ ] All conditions have `if` expression
- [ ] Has `else: true` as default case
- [ ] All branches lead to existing steps

### Parallel
- [ ] Each branch has its own `steps` array
- [ ] Each branch ends with terminal
- [ ] `join` strategy is specified
- [ ] `on_complete.next` is defined

### Context
- [ ] All used variables declared in `context`
- [ ] Variables set before use via `on_success.set`
- [ ] Template syntax: `"{{ context.var }}"`

### Transitions
- [ ] All `next` values reference existing steps
- [ ] No circular references that bypass terminals
- [ ] Error paths lead to terminal or recovery
