# 0006: Bridge-side bounded MCP completion

- Status: Superseded by [ADR 0007](0007-mcp-agent-delivery-defaults.md)
- Date: 2026-11-03
- Related: [Issue #205](https://github.com/TX230/winproc-tui/issues/205)
- Contract: [MCP integration](../mcp-integration-specification.md#bounded-completion-and-first-page-retrieval)

## Context and constraints

Model-driven clients spend seconds between dependent tool calls even when native queries take milliseconds. Starting a job, polling it, and reading a first page amplify that cost across native investigations, saved-log work and condition waits. Application operations have the same completion-query overhead but must retain identity/revision checks, idempotency and local confirmation. The shared application dispatcher must remain available to other connections and TUI publication.

## Decision

Add opt-in bounded completion in the stdio bridge using existing application-owned job and operation APIs. Dispatch the requested action once, poll only its returned ID for at most five seconds, and optionally read the first frozen capture page after successful completion. Return running/pending work at the budget limit and stop waiting immediately when local confirmation is required. Keep asynchronous defaults and existing data ownership, cancellation, connection loss and resource bounds.

## Alternatives

- Waiting inside the shared query thread: starves other clients and invalidation handling.
- Always waiting or changing existing result shapes: breaks clients relying on prompt asynchronous starts.
- Generic scripts/batches with interpolated dependencies: expands authorization and error semantics beyond the measured completion problem.
- Optimizing only handle enumeration: cannot remove repeated model/client waits, and benefits only one interface category.

## Consequences

Common short jobs return a first useful result in one client turn. Slow jobs still require subsequent status calls. Each bridge remains serialized during its bounded wait, while independent connections stay responsive. The bridge uses a few extra inexpensive internal polls; it does not duplicate collectors or application state. Full page envelopes can still be large, so clients should use projection, selection and response-size-bounded pages. This reduces interface overhead, without promising a bound on model inference or service latency.
