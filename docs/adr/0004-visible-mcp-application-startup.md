# 0004: Visible application startup from the MCP bridge

- Status: Accepted
- Date: 2026-11-01
- Supersedes: [ADR 0001](0001-local-mcp-bridge.md)
- Related: [Issue #202](https://github.com/TX230/winproc-tui/issues/202)
- Specification: [MCP integration](../mcp-integration-specification.md), section 3

## Context and constraints

An application launched inside an automation tool's private terminal is invisible to the desktop user but holds the normal single-instance marker, preventing ordinary interactive startup. Requiring manual startup also makes initial MCP use unnecessarily indirect. The maintainer requests automatic startup in a visible Windows Terminal window. MCP must retain stdio compatibility, local named pipes, one distributed executable, and no TCP/IP.

## Decision

Retain ADR 0001's stdio bridge, SDK protocol baseline, authenticated named pipe, bounded asynchronous adapter and application-owned collection/history/persistence. Add lazy visible startup on the first validated tool call. Static discovery remains independent of application availability.

Reuse a running application. Only an absent pipe and absent application marker allow launch, coordinated by a separate Windows-session-local marker. Read saved MCP availability without migration/writes and respect disablement. Launch the bridge's verified executable launch path through the Windows Terminal app execution alias in a new window with isolated stdio, then wait for authenticated readiness under a bounded deadline. Never request elevation or fall back to hidden hosting. Leave the application running after client disconnect.

## Alternatives

- Always require manual startup: avoids launch coordination but leaves the requested first-use problem unresolved.
- Automatically launch in a hidden terminal: reproduces the invisible single-instance obstruction.
- Launch during initialization/discovery: opens a window merely when a client inspects available tools.
- Independent headless collector: changes data ownership, duplicates collection and does not solve the requested visible interaction.
- HTTP or a separate bridge executable: retains the transport and packaging disadvantages recorded in ADR 0001.

## Consequences

The human can see and operate the same application, including local confirmations. Automatic startup requires Windows Terminal and its execution alias; manual startup remains usable without them. Normal startup choices and saved settings remain authoritative and may require human input. A marker cannot distinguish a disabled listener from startup delay or listener failure, so errors describe uncertainty accurately. Default Terminal profile behavior can affect startup; peer integrity checks remain mandatory.

The ordinary application mutex prevents duplicate samplers. A concurrent manual launch or bridge crash during the handoff can leave an extra rejected window; the bridge does not manage unrelated windows. Shared human/MCP investigation-state conflicts remain governed by revisions and application command serialization from ADR 0003; this change does not split investigation ownership.

Windows Terminal invocation follows the [official command-line interface](https://learn.microsoft.com/en-us/windows/terminal/command-line-arguments), including explicit new-window selection.
