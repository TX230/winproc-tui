# 0007: Agent-facing MCP delivery defaults

- Status: Accepted
- Date: 2026-11-03
- Supersedes: [ADR 0006](0006-mcp-bounded-completion.md)
- Related: [Issue #205](https://github.com/TX230/winproc-tui/issues/205)
- Contract: [MCP integration](../mcp-integration-specification.md#bounded-completion-and-first-page-retrieval)

## Context and constraints

Opt-in completion removed some client round trips but required models to assemble wait/read options and manually release source and derived captures. Native measurements still showed seconds between inexpensive MCP calls. The maintainer requested interfaces usable from ordinary investigation requests without making the agent plan mechanical lifecycle steps. All interface categories are in scope; identity checks, revisions, idempotency, local confirmation, application ownership and resource limits must remain intact. This unreleased contract needs an explicit route for existing retained-result clients.

## Decision

Make the normal bridge use bounded completion, automatic first-page retrieval and one-pass capture cleanup by default. Return exact continuation tool/arguments when a job or page remains. Preserve full capture metadata and independent process/log references. Provide explicit retention for repeated inspection and `mcp-bridge --raw` for the earlier asynchronous/caller-owned lifecycle. Keep all 26 operations available; do not add overlapping purpose aliases or generic scripts/batches.

Keep delivery bookkeeping in the connection-owning bridge. The application still owns snapshots, captures, jobs, loaded logs and action authorization. Default metric reads to the system and infer live/log source only when source is omitted. Enumerate the existing metric identifiers in schemas with a runtime parity test and return safe committed-schema constraints on argument errors. Do not synthesize state-changing intent, operation IDs or expected revisions.

## Alternatives

- Keep completion opt-in and supply longer prompts: leaves lifecycle planning with users/models and does not remove cleanup turns.
- Hide or remove low-level tools: reduces available investigation/control capabilities.
- Add many overlapping convenience tools: increases tool-selection ambiguity and maintains duplicate entry points.
- Add arbitrary dependency batches: expands authorization, cancellation and unknown-outcome semantics beyond this change.
- Make all reads permanently retained: still requires release decisions and can exhaust capture capacity.

## Consequences

Common investigations finish with fewer client decisions and tool calls. Large or slow results still require a bounded, explicit continuation; model-service latency remains. One-pass results cannot be reread after consumption. Clients needing independent frozen views must opt into retention or raw mode. Loaded logs retain their explicit close lifecycle because subsequent queries depend on them. The shared query worker never waits for client delivery, and a connection loss retains the established application-owned operation rules.
