# 0001: Local MCP bridge and application ownership

- Status: Superseded by [ADR 0004](0004-visible-mcp-application-startup.md) on 2026-11-01 (visible application startup; transport and collection ownership retained)
- Date: 2026-10-31
- Related: [Issue #202](https://github.com/TX230/winproc-tui/issues/202)
- Specification: [MCP integration](../mcp-integration-specification.md)

## Context and constraints

General MCP clients support stdio. The application must remain a single portable executable and MCP communication must not use TCP/IP. The interactive application owns sampling, history, recording, and user settings. Slow collectors must not block its event loop.

## Decision

Run the stdio MCP server as the `mcp-bridge` subcommand in a separate process of the same executable. Use the official Rust MCP SDK with explicitly negotiated protocol revision 2025-11-25. Enable only its server and stdio features, not HTTP transports. Connect to the running application through versioned, bounded, length-prefixed JSON over a local Windows named pipe. Restrict the pipe by logon identity and integrity level and reject remote clients.

Use a dedicated asynchronous IPC runtime and bounded channels. The application thread applies typed commands and publishes immutable data; background workers own potentially blocking collection. Neither terminal navigation nor dialog state is an API dispatch mechanism. Enabling the listener alone performs no additional collection.

## Alternatives

- HTTP/loopback: violates the required MCP communication constraint.
- Named-pipe-only MCP: requires nonstandard support in every client.
- Separate bridge executable: adds packaging/version coordination without a runtime benefit.
- Independent sampling in the bridge: produces different histories and duplicates collection cost.
- Handwritten MCP lifecycle: duplicates protocol compatibility and cancellation machinery already maintained by the SDK.

## Consequences

Clients register one executable subcommand. Runtime modes must dispatch before terminal initialization and the interactive single-instance mutex. Backend loss invalidates connection-owned references. The bridge can still expose its tool catalog when the application is absent. Updating the on-disk executable does not update an already-running backend, so IPC version negotiation remains necessary.
