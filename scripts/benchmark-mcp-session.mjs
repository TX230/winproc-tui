import {spawn} from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import {collect} from './mcp-session-telemetry.mjs';

// Usage: node scripts/benchmark-mcp-session.mjs <codex-cli.js> <exe> <output-dir> <model>
//   [normal|direct] [high|low|alternating] [turns] [full|direct-only] [default|fast|alternating] [status|cpu]
// Creates an ephemeral Codex conversation; every turn requests fresh read-only status or CPU data.
// The alternating tier mode consumes extra usage on its Fast turns. Select it explicitly.
const [cliFile, exeFile, outDir, model, style = 'normal', effort = 'high',
  countText = '4', surface = 'full', tier = 'default', query = 'status'] = process.argv.slice(2);
const count = Number(countText);
if (!cliFile || !exeFile || !outDir || !model || !['normal', 'direct'].includes(style)
    || !['high', 'low', 'alternating'].includes(effort) || !Number.isInteger(count) || count < 1 || count > 20
    || !['full', 'direct-only'].includes(surface) || !['default', 'fast', 'alternating'].includes(tier) || !['status', 'cpu'].includes(query)) {
  throw Error('Invalid benchmark arguments; see the usage comment.');
}
const folder = path.resolve(outDir);
if (fs.existsSync(folder) && fs.readdirSync(folder).length) throw Error('Choose an empty output directory.');
fs.mkdirSync(folder, {recursive: true});
const cli = path.resolve(cliFile), exe = path.resolve(exeFile);
const proxy = path.join(import.meta.dirname, 'trace-mcp-stdio.mjs');
for (const file of [cli, exe, proxy]) if (!fs.existsSync(file)) throw Error('Missing input: ' + file);
const collector = await collect(folder);
const portable = file => file.replaceAll('\\', '/');
const config = [
  'model=' + JSON.stringify(model),
  'model_reasoning_effort=' + JSON.stringify(effort === 'alternating' ? 'low' : effort),
  // Isolate this benchmark from unrelated MCP startup without editing user configuration.
  'mcp_servers={winproc={command=' + JSON.stringify(portable(process.execPath))
    + ',args=' + JSON.stringify([portable(proxy), portable(exe), portable(path.join(folder, 'wire.jsonl'))])
    + ',omit_tools_from=' + JSON.stringify(surface === 'direct-only' ? ['deferred', 'code_mode'] : []) + '}}',
  ...collector.config,
];
const args = [cli, 'app-server', '--stdio'];
for (const setting of config) args.push('-c', setting);
const child = spawn(process.execPath, args, {stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true});
const now = () => performance.timeOrigin + performance.now();
const start = now(), pending = new Map(), events = [], waiters = [];
let seq = 0, buffer = '', closed = false, activeTurn = null;
let stderrBuffer = '';
child.stderr.on('data', bytes => {
  fs.appendFileSync(path.join(folder, 'stderr.log'), bytes);
  stderrBuffer += bytes.toString();
  let end;
  while ((end = stderrBuffer.indexOf('\n')) >= 0) {
    const line = stderrBuffer.slice(0, end); stderrBuffer = stderrBuffer.slice(end + 1);
    let record; try { record = JSON.parse(line); } catch { continue; }
    if (activeTurn && /429/.test(record.fields?.sampling_error ?? record.fields?.message ?? '')) {
      // Built-in providers cannot be overridden. Interrupt a rate-limited test instead
      // of changing provider identity/authentication or waiting out its retry loop.
      const turn = activeTurn; activeTurn = null;
      void rpc('turn/interrupt', turn).catch(() => {});
    }
  }
});
child.stdout.setEncoding('utf8');
child.stdout.on('data', text => {
  buffer += text;
  let end;
  while ((end = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, end);
    buffer = buffer.slice(end + 1);
    let message;
    try { message = JSON.parse(line); } catch { continue; }
    const record = {epoch_ms: now(), message};
    events.push(record);
    fs.appendFileSync(path.join(folder, 'events.jsonl'), JSON.stringify(record) + '\n');
    if (message.id != null && !message.method) {
      const waiting = pending.get(String(message.id));
      if (waiting) {
        pending.delete(String(message.id));
        clearTimeout(waiting.timer);
        if (message.error) waiting.reject(Error(JSON.stringify(message.error)));
        else waiting.resolve(message.result);
      }
    } else if (message.id != null && message.method) {
      child.stdin.write(JSON.stringify({id: message.id, error: {
        code: -32000, message: 'Unexpected approval/input request in read-only benchmark',
      }}) + '\n');
    }
    for (const waiting of [...waiters]) if (waiting.test(message)) {
      waiters.splice(waiters.indexOf(waiting), 1);
      clearTimeout(waiting.timer);
      waiting.resolve(record);
    }
  }
});
function fail(error) {
  closed = true;
  for (const waiting of pending.values()) { clearTimeout(waiting.timer); waiting.reject(error); }
  for (const waiting of waiters) { clearTimeout(waiting.timer); waiting.reject(error); }
  pending.clear();
  waiters.length = 0;
}
child.on('error', fail);
child.on('exit', code => fail(Error('App server exited: ' + code)));
function rpc(method, params) {
  const id = ++seq;
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => { pending.delete(String(id)); reject(Error('RPC timeout: ' + method)); }, 90000);
    pending.set(String(id), {resolve, reject, timer});
    child.stdin.write(JSON.stringify({id, method, params}) + '\n');
  });
}
function notice(test) {
  return new Promise((resolve, reject) => {
    const waiting = {test, resolve, reject, timer: setTimeout(() => {
      waiters.splice(waiters.indexOf(waiting), 1);
      reject(Error('Turn completion timeout'));
    }, 90000)};
    waiters.push(waiting);
  });
}
const prompts = {
  normal: 'Use winproc MCP to get application status once now. Make a fresh tool call for this turn rather than reusing previous results. Return only activity and stale. No shell, files, web, skills, subagents or state changes.',
  direct: 'Call winproc_get_status with {} as your first action this turn. Make a fresh call; do not reuse the prior result. Do not emit a preliminary acknowledgement or plan. Return only activity and stale. No shell, files, web, skills, subagents or state changes.',
};
const cpuPrompts = {
  normal: 'Use winproc MCP to read the current system CPU percentage once now. Make a fresh tool call. Return only the CPU percentage and sample timestamp. No shell, files, web, skills, subagents or state changes.',
  direct: 'Call winproc_get_metrics with {"metric_ids":["cpu_average_percent"]} as your first action. Make a fresh call and return only CPU percentage and sample timestamp. No preliminary acknowledgement, shell, files, web, skills, subagents or state changes.',
};
const turns = [];
try {
  await rpc('initialize', {clientInfo: {name: 'winproc-session-benchmark', version: '1'}, capabilities: {experimentalApi: true}});
  child.stdin.write(JSON.stringify({method: 'initialized', params: {}}) + '\n');
  const created = await rpc('thread/start', {model, cwd: process.cwd(), sandbox: 'read-only', approvalPolicy: 'on-request', ephemeral: true});
  if (created.thread.ephemeral !== true) throw Error('Benchmark thread is not ephemeral.');
  const threadId = created.thread.id;
  for (let index = 0; index < count; index++) {
    const requested = now(), eventIndex = events.length;
    const done = notice(message => message.method === 'turn/completed' && message.params.threadId === threadId);
    // Avoid an unhandled rejection if turn/start itself fails before completion.
    done.catch(() => {});
    const selectedTier = tier === 'alternating' ? (index >= 2 && index % 2 === 0 ? 'fast' : 'default') : tier;
    const selectedEffort = effort === 'alternating' ? (index % 2 === 0 ? 'low' : 'high') : effort;
    const response = await rpc('turn/start', {threadId, input: [{type: 'text', text: (query === 'cpu' ? cpuPrompts : prompts)[style]}], effort: selectedEffort, serviceTier: selectedTier});
    activeTurn = {threadId, turnId: response.turn.id};
    const completed = await done;
    activeTurn = null;
    const turn = completed.message.params.turn;
    const wireFile = path.join(folder, 'wire.jsonl');
    const wire = fs.existsSync(wireFile) ? fs.readFileSync(wireFile, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse) : [];
    const calls = wire.filter(record => record.event === 'client_request' && record.method === 'tools/call'
      && record.epoch_ms >= requested && record.epoch_ms <= completed.epoch_ms);
    const items = events.slice(eventIndex).filter(record => record.message.method === 'item/completed');
    const mcp = items.map(record => record.message.params.item).filter(item => item.type === 'mcpToolCall');
    const data = mcp[0]?.result?.structuredContent?.data;
    const first = calls[0];
    const reply = first && wire.find(record => record.event === 'response_flushed' && record.method === 'tools/call' && record.id === first.id);
    const stderr = fs.existsSync(path.join(folder, 'stderr.log')) ? fs.readFileSync(path.join(folder, 'stderr.log'), 'utf8') : '';
    const retryEvents = stderr.split('\n').flatMap(line => { try { return [JSON.parse(line)]; } catch { return []; } })
      .filter(record => (record.fields?.turn_id === response.turn.id || (Date.parse(record.timestamp) >= requested && Date.parse(record.timestamp) <= completed.epoch_ms))
        && /429/.test(record.fields?.sampling_error ?? record.fields?.message ?? ''));
    const expectedTool = query === 'cpu' ? 'winproc_get_metrics' : 'winproc_get_status';
    const result = {
      index, requested_tier: selectedTier, requested_effort: selectedEffort, turn_id: response.turn.id,
      turn_status: turn.status, turn_error: turn.error, retry_429_events: retryEvents.length,
      service_clean: turn.status === 'completed' && retryEvents.length === 0,
      mcp_ms: reply ? Number(BigInt(reply.ns) - BigInt(first.ns)) / 1e6 : null,
      after_mcp_ms: reply ? completed.epoch_ms - reply.epoch_ms : null,
      requested_epoch_ms: requested, completed_epoch_ms: completed.epoch_ms,
      first_call_ms: calls[0] ? calls[0].epoch_ms - requested : null,
      total_ms: completed.epoch_ms - requested, calls: calls.map(call => call.tool),
      mcp_items: mcp.map(item => ({tool: item.tool, status: item.status, error: item.error, result: item.result})),
      preamble: items.some(record => record.message.params.item.type === 'agentMessage'
        && record.epoch_ms < (calls[0]?.epoch_ms ?? Infinity)),
      correct: turn.status === 'completed' && calls.length === 1 && calls[0].tool === expectedTool && mcp.length === 1
        && mcp[0].status === 'completed' && !mcp[0].error
        && (query === 'status' ? typeof data?.activity === 'string' && typeof data?.stale === 'boolean'
          : mcp[0].result?.structuredContent?.source === 'live'
            && data?.samples?.[0]?.target?.system === true
            && Number.isFinite(data?.samples?.[0]?.values?.cpu_average_percent)),
    };
    turns.push(result);
    fs.writeFileSync(path.join(folder, 'summary.json'), JSON.stringify({model, style, effort, surface, tier, query, interrupt_on_429: true, isolated_mcp_server: true,
      ephemeral: true, session_elapsed_ms: now() - start, turns}, null, 2) + '\n');
    if (!result.correct || result.retry_429_events) throw Error('Incorrect or service-retried workflow; inspect summary.json and stderr.log.');
    console.log(JSON.stringify({index, tier: selectedTier, effort: selectedEffort, first_call_ms: result.first_call_ms, mcp_ms: result.mcp_ms, after_mcp_ms: result.after_mcp_ms, total_ms: result.total_ms}));
  }
} finally {
  child.stdin.end();
  if (!closed) {
    let timer;
    await Promise.race([new Promise(resolve => child.once('exit', resolve)),
      new Promise(resolve => { timer = setTimeout(resolve, 5000); })]);
    clearTimeout(timer);
    if (!closed) child.kill();
  }
  await collector.close();
}
