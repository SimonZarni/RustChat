# 0003. Garage for object storage

- **Status:** Accepted; S3-compatible adapter and initial attachment flow implemented; Garage deployment remains operator-provisioned
- **Date:** 2026-11-14

## Context

Attachments need S3-compatible object storage that can be self-hosted. The README and `ARCHITECTURE.md` named SeaweedFS while the PRD named Garage, so the documents disagreed.

## Decision

Use [Garage](https://garagehq.deuxfleurs.fr/) as the default object store. The backend talks to it only through an S3-compatible API behind an application port, so the store can be swapped.

## Alternatives considered

- **SeaweedFS (Apache-2.0):** mature and feature-rich, with several components to run; its scale-oriented design is more than a single-node deployment needs.
- **Other S3-compatible stores (for example MinIO):** not evaluated in depth for this decision.

## Consequences

Simpler to operate on one node, and consistent with a Rust, small-footprint stack. Garage is AGPL-3.0. Running it unmodified as a separate service does not extend its licence to RustChat, but this must be confirmed and recorded in [DEPENDENCIES.md](../DEPENDENCIES.md) before adoption (its licence entry there is not yet verified). Revisit if Garage lacks a needed feature such as presigned uploads at the size limits required, or if its licence becomes a problem for operators.
