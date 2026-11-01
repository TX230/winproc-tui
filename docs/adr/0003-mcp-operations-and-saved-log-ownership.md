# 0003: Application-owned MCP operations and independent saved logs

- Status: Accepted
- Date: 2026-10-31
- Related: [Issue #202](https://github.com/TX230/winproc-tui/issues/202)
- Contract: [MCP integration](../mcp-integration-specification.md)

## Context and constraints

Remote requests may overlap human edits, disconnect while awaiting approval, or lose their reply after an accepted mutation. Saved-log analysis must not switch the human's TUI activity or expose recorded identities to live collectors. Native work and file providers may block, and compact recording definitions can expand substantially in memory.

## Decision

Keep a bounded operation ledger in application-owned state, independently of the listener. The query adapter submits immutable descriptions and resolved identities; the application serializes mutation with its normal state transitions. A process-instance revision namespace and monotonically increasing revision cover tracking, profiles, and collection authorization. Conservative conflicts across these domains are permitted. Profile identifiers and Recording session identifiers are opaque and session-local.

Retain at most 1,024 operation outcomes for 24 hours. Identical operation IDs and arguments return the original result, including after bridge reconnection; changed arguments conflict. One application-wide operation may await execution or local confirmation. Required confirmation uses a dedicated scrollable TUI overlay, expires after 60 seconds, and rechecks revision, activity, and lifetime. Disconnect or MCP disable cancels pending approval. Accepted Recording and .NET authorization belong to the application. A mutex serializes final acceptance with disconnect bookkeeping; no lock is held while waiting for human input.

Use connection-owned asynchronous jobs for process image metadata, directory listing, log loading, and condition waits as well as native investigation. Loaded logs have independent reference maps and histories, share the capture count and result budget, and never enter TUI Log view. Reuse the existing schema-v2/v3 reconstruction with an external-load budget and deadline. Reserve memory conservatively before records and referenced definitions are expanded; reject excess instead of truncating histories. An invalid unfinished final line is reported as interrupted, while an invalid complete line fails the load.

## Alternatives

- Mutate App from the IPC thread: introduces ownership races and bypasses local confirmation rules.
- Use caller-supplied confirmation flags: bypasses readiness and retention decisions owned by the human.
- Tie accepted Recording to the bridge lifetime: an incidental connection loss would stop the reproduction.
- Load saved logs through the TUI Log-view action: disrupts the human's current investigation and sampling.
- Bound only source-file size: compact repeated definitions can expand beyond the retained-memory budget.

## Consequences

Clients poll operation or job state and obtain fresh revisions after conflicts. The shared revision deliberately rejects some otherwise independent simultaneous edits. The existing application-owned recording/configuration write paths retain their existing I/O behavior; native investigation and log loading stay on bounded workers. Conservative log reservations may reject a source that would fit under a precise heap measurement. MCP continues to use only stdio and local named pipes; configured recording directories may be network paths.
