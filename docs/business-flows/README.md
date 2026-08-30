# mailing — Business Flows (the oracle)

One doc per business flow this module owns. Each flow captures **actors, preconditions, main path,
business rules, alternate/failure paths, postconditions** — in business terms — and links to the
executable oracle (golden-case tests) that proves it. This is authored *first*: it defines
correctness before the Rust is written.

Suggested files (delete/rename to fit the module):

- `<flow>.md` — one per real flow (e.g. `onboarding.md`, `checkout.md`).
- `golden-cases.md` — the exact expected results (numbers, statuses, error codes) that mirror the
  tests one-to-one.

BDD scenarios live in `tests/features/*.feature`; the executable oracle lives in `tests/*.rs`.
Keep the three in step: flow doc ↔ feature ↔ golden-case test.
