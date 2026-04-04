# sc-observability Schema Pointer

## 1. Ownership

`sc-observability` owns its own event, query, and transport schemas.

ATM should reference those schemas but must not redefine them locally as if ATM
owned them.

## 2. Repository Pointer

The owning repository referenced by ATM planning docs is:

- `/Users/randlee/Documents/github/sc-observability`

Related ATM references:

- `docs/archive/obs-gap-analysis.md`
- `docs/requirements.md`
- `docs/architecture.md`

## 3. Local Rule

If ATM needs to reference `sc-observability` schema contracts in future design
docs, those references should live alongside:

- [`claude-code-message-schema.md`](./claude-code-message-schema.md)
- [`atm-message-schema.md`](./atm-message-schema.md)

but should remain pointers and ownership notes, not copied schema definitions.
