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

The maintained sources for Issue #202 are:

- [`mcp-tools.json`](mcp-tools.json): tool names, descriptions, annotations and input schemas. An optional `outputDataSchema` defines only that tool's successful `data` object.
- [`mcp-output-envelope.schema.json`](mcp-output-envelope.schema.json): the shared success/error output schema, including the default unconstrained `data` object.

`src/mcp/catalog.rs` embeds both files and assembles the catalog once at runtime. It clones the common envelope for each tool, replaces the success `data` schema when `outputDataSchema` is present, and publishes the assembled `outputSchema` through `tools/list`. The source-only `outputDataSchema` field is not published. Do not duplicate `outputSchema` in tool definitions. There is no external schema lookup or generated catalog to maintain.

All 26 tools currently use the default unconstrained success `data` object; tool-specific result fields and capture-row shapes are not yet fully schematized. Source deduplication leaves the published tool contracts unchanged. Use MCP `tools/list` to inspect the complete expanded schemas. Catalog and stdio tests check expansion, tool isolation, error validation and discovery.

These definitions are not a released compatibility promise. The complete contract and conformance requirements are owned by [MCP integration](../mcp-integration-specification.md).
