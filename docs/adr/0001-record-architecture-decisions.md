# 0001. Record architecture decisions

- **Status:** Accepted
- **Date:** 2026-11-14

## Context

Phase 0 of the PRD asks for "approved PRD and architecture decisions". The existing documents describe the intended design but not why choices were made or which alternatives were rejected. Contributors, human and automated, need a place to see that and to propose changes.

## Decision

Keep Architecture Decision Records in `docs/adr/`, one file per decision, in the format described in [README.md](README.md). Significant architectural, security, protocol, dependency and licensing changes get an ADR.

## Alternatives considered

- **Only the long-form documents.** They say what the design is, but history of the reasoning is lost as they are edited.
- **Issue tracker or wiki.** Easy to start, but separated from the code and its review.

## Consequences

A small writing cost per significant decision, in exchange for a reviewable history that lives with the code. Decisions already made were recorded retrospectively (ADRs 0002 to 0008).
