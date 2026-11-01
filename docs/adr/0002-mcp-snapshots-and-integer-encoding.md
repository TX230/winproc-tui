# 0002: Immutable MCP views and exact integer metrics

- Status: Accepted
- Date: 2026-10-31
- Related: [Issue #202](https://github.com/TX230/winproc-tui/issues/202)
- Specification: [MCP integration](../mcp-integration-specification.md)

## Context

MCP queries may outlive a sampling update or terminal selection. Reusing displayed data would incorrectly freeze remote measurements during display pause. Reusing terminal inspection sessions would allow concurrent clients to overwrite human investigation state. Large unsigned measurements also exceed the exact integer range of common JSON clients.

## Decision

Publish an immutable view of the latest applied snapshot and copy-on-write history containers. Resolve queries on a separate thread. Bind opaque references and frozen pagination captures to a single authenticated pipe connection. Do not pin live histories merely because a client retains a process list. Retained lifetime discovery reads the history identity index independently of Ghost Row presentation.

Encode unsigned integer metrics as decimal strings and finite percentages as JSON numbers. Compute integer summary extrema without floating-point conversion. Encode integer means as an exact rational object with decimal-string `numerator` and `denominator`; this avoids inventing a rounding policy. A missing sample value contributes to the missing-value count; time without a stored sample does not. Return endpoint deltas only when both requested endpoints have available values.

Run native collection on bounded workers. Cancellation prevents publication; native cleanup may continue, retaining its worker slot so repeated cancellation cannot create unlimited threads. Hold and revalidate a query-only process handle around targeted capture.

## Alternatives

- Query display accessors: incorrectly inherits display pause and loaded-log state.
- Query mutable application state directly from IPC: couples slow client work to the event loop and complicates ownership.
- Convert all values to floating point: loses integer precision, including extrema and deltas.
- Kill a native worker thread on timeout: unsafe resource and API cleanup.

## Consequences

Readers may observe the last published sample, with its actual timestamp and freshness. Independent captures consume explicit memory and lifetime budgets. Clients must format rational integer means when they need a decimal display. Cancellation does not imply that a blocking Windows call has already returned.
