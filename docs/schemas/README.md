# Recording Schemas

[`recording-v3-line.schema.json`](recording-v3-line.schema.json) is a JSON Schema Draft 2020-12 description of one non-empty JSON Lines record emitted by the current schema-v3 writer.

Validate each line independently against the schema. Cross-record rules remain normative in [metrics.md](../metrics.md), including:

- the session record is first;
- process and GPU definitions precede frames that reference their IDs;
- one file uses one fixed session scope and aggregation interval;
- the end record is optional after interruption or failure.

The schema accepts the shorter process metric arrays supported by the schema-v3 reader for backward compatibility. Other fixed-order arrays describe the current writer shape exactly.

When a schema-v3 field or array position changes, update all of the following together:

1. `src/app/log_format.rs`;
2. `docs/metrics.md`;
3. `docs/schemas/recording-v3-line.schema.json`;
4. writer, reader, and schema-parity tests.

## MCP tool contracts

[`mcp-tools.json`](mcp-tools.json) records the input schemas and common success/error output-envelope schemas for Issue #202. Tool-specific success data remains an unconstrained object; the catalog does not yet define every result field or capture-row shape. The bridge loads this catalog and advertises all 26 implemented adapters. These definitions are not a released compatibility promise. The complete contract and conformance requirements are owned by [MCP integration](../mcp-integration-specification.md).
