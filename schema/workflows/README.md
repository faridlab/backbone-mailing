# mailing Workflows

Declarative specs of the module's multi-step sagas. The hand-authored Rust is the executable
truth; these YAML files document the intended orchestration and are the readable companion to the
golden cases in `docs/business-flows/`.

| Workflow | Saga | Implemented in | Proven by |
|----------|------|----------------|-----------|
| `example.workflow.yaml` | (replace with your saga) | `src/application/service/…` | `tests/…` |

Single-entity status transitions belong in `schema/hooks/*.hook.yaml` as state machines, not here.
Use a workflow only for a **multi-step** saga (e.g. "create A then B atomically", "A → B → C").
Delete `example.workflow.yaml` if this module has no sagas.
