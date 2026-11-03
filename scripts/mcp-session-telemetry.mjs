import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

// Diagnostic-only OTLP endpoint. MCP continues to use stdio and the local named pipe.
// Never retain prompts, tool arguments/results, arbitrary attributes, headers or resource metadata.
export async function collect(folder) {
  const allowed = new Set(['event.name', 'event_name', 'event.timestamp', 'startup.phase', 'model',
    'model_reasoning_effort', 'service_tier', 'duration_ms', 'latency_ms', 'status', 'status_code',
    'http.status_code', 'http.response.status_code', 'ttft_ms', 'input_tokens', 'output_tokens',
    'input_token_count', 'output_token_count', 'tool_token_count', 'cached_token_count',
    'cached_input_tokens', 'cache_write_token_count', 'reasoning_token_count', 'attempt', 'retry_count',
    'response_type', 'message_type', 'event.kind', 'target', 'request_method', 'auth.connection_reused',
    'tool_name', 'tool_namespace', 'output_truncated', 'success']);
  const sanitize = attrs => (attrs ?? []).filter(item => allowed.has(item.key));
  const server = http.createServer((request, response) => {
    const chunks = [];
    request.on('data', bytes => chunks.push(bytes));
    request.on('end', () => {
      try {
        const body = JSON.parse(Buffer.concat(chunks));
        const records = [];
        for (const resource of body.resourceLogs ?? []) for (const scope of resource.scopeLogs ?? []) {
          for (const log of scope.logRecords ?? []) records.push({kind: 'log', time_ns: log.timeUnixNano,
            attrs: sanitize(log.attributes)});
        }
        for (const resource of body.resourceSpans ?? []) for (const scope of resource.scopeSpans ?? []) {
          for (const span of scope.spans ?? []) records.push({kind: 'span', name: span.name,
            start_ns: span.startTimeUnixNano, end_ns: span.endTimeUnixNano, attrs: sanitize(span.attributes)});
        }
        if (records.length) fs.appendFileSync(path.join(folder, 'otel.jsonl'), records.map(record => JSON.stringify(record)).join('\n') + '\n');
      } catch (error) {
        fs.appendFileSync(path.join(folder, 'otel-error.txt'), String(error) + '\n');
      }
      response.writeHead(200, {'content-type': 'application/json'});
      response.end('{}');
    });
  });
  await new Promise((resolve, reject) => { server.once('error', reject); server.listen(0, '127.0.0.1', resolve); });
  const endpoint = 'http://127.0.0.1:' + server.address().port;
  return {
    close: () => new Promise(resolve => server.close(resolve)),
    config: ['otel.exporter.otlp-http.endpoint=' + JSON.stringify(endpoint + '/v1/logs'),
      'otel.exporter.otlp-http.protocol="json"',
      'otel.trace_exporter.otlp-http.endpoint=' + JSON.stringify(endpoint + '/v1/traces'),
      'otel.trace_exporter.otlp-http.protocol="json"', 'otel.log_user_prompt=false',
      'otel.log_agent_responses=false', 'otel.tool_result.max_bytes=0'],
  };
}
