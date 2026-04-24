# tubbie — Documentation

Conceptual and explanatory documentation for the tubbie project.
This is not API reference; it captures *why* things are built the way they are.

## Structure

- [`architecture.md`](architecture.md) — how the crates fit together, data flow, trait seams, security surfaces, and testing strategy
- [`ADR/`](ADR/README.md) — Architectural Decision Records (primarily for AI agents consuming prior decisions)

## Guidelines

- Every non-obvious design choice, subsystem, or concept earns a document here.
- Filenames are kebab-case slugs; no numeric prefixes (numbers live in the ADR index table only).
- Link new documents from this README.
