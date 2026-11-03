import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {pathToFileURL} from 'node:url';

// Python-free MCP lifecycle test. Requires an isolated test TUI configured with
// the marker Tracking List entry and the supplied Recording directory.
// node scripts/test-mcp-recording-stop.mjs <exe> <tui-pid> <sdk-root> <recording-dir> <results.json>
const [exe, pidText, sdkRoot, directory, output] = process.argv.slice(2);
if (!exe || !pidText || !sdkRoot || !directory || !output) throw Error('Expected executable, owned TUI PID, SDK root, Recording directory and result path');
const {Client} = await import(pathToFileURL(path.resolve(sdkRoot,'dist/esm/client/index.js')));
const {StdioClientTransport} = await import(pathToFileURL(path.resolve(sdkRoot,'dist/esm/client/stdio.js')));
const client = new Client({name:'winproc-recording-stop-test',version:'1'},{capabilities:{}});
const calls = [], sessions = new Set();
await client.connect(new StdioClientTransport({command:exe,args:['mcp-bridge'],stderr:'pipe'}));
async function call(name,args={}) {
  const started = performance.now();
  const reply = await client.callTool({name:'winproc_'+name,arguments:args});
  const result = reply.structuredContent;
  assert.deepEqual(JSON.parse(reply.content.find(c=>c.type==='text').text),result);
  calls.push({name,args,ms:performance.now()-started,result});
  assert(!reply.isError,JSON.stringify(result));return result;
}
async function finish(value) {
  const deadline = performance.now()+10000;
  while(['pending','running'].includes(value.data.state)) {
    assert(performance.now()<deadline,JSON.stringify(value));
    const next = value.data.next_action;
    value = next ? await call(next.tool.replace('winproc_',''),next.arguments)
      : await call('get_operation',{operation_id:value.data.operation_id,wait_ms:5000});
  }
  assert.notEqual(value.data.state,'pending_confirmation','MCP stop must not require human confirmation');
  return value;
}
async function start() {
  const tracking = await call('get_tracking');
  const result = await finish(await call('start_recording',{interval_seconds:10,operation_id:randomUUID(),expected_revision:tracking.data.revision}));
  assert.equal(result.data.state,'succeeded');
  sessions.add(result.data.result.recording_session_id);return result.data.result;
}
try {
  const tools = (await client.listTools()).tools;
  const stop = tools.find(t=>t.name==='winproc_stop_recording');assert(stop.description.includes('without local confirmation'));
  assert.equal((await call('get_status')).data.activity,'live');
  const tracking = await call('get_tracking');
  assert.deepEqual(tracking.data.tracked_names,['winproc-mcp-stop-test-207.exe'],'Use an isolated configured test TUI');
  const target = await call('list_processes',{filter:{pid:Number(pidText)}});assert(target.data.rows.some(row=>row.pid===Number(pidText)));
  const before = (await call('get_recording')).data;assert.equal(before.active,false);assert.equal(before.output_configured,true);
  const first = await start();
  await new Promise(resolve=>setTimeout(resolve,1250));
  const args = {recording_session_id:first.recording_session_id,operation_id:randomUUID()};
  const stopped = await finish(await call('stop_recording',args));
  assert.equal(stopped.data.state,'succeeded');assert.equal(stopped.data.result.stopped,true);
  assert.equal((await call('get_recording')).data.active,false);
  const listing = await finish(await call('list_logs'));
  const row = listing.data.result_page.data.rows.find(row=>row.log_id===first.output_log_id);assert(row);
  assert.equal(path.basename(row.name),row.name);
  const filename = path.join(directory,row.name), bytes = fs.readFileSync(filename);
  const records = bytes.toString('utf8').trimEnd().split('\n').map(line=>JSON.parse(line));
  assert.equal(records[0].s.interval,10);assert.equal(records.at(-1).e[1],'stopped');
  const frames = records.filter(record=>record.f);assert.equal(frames.length,1,'Partial final aggregation window must be flushed');
  const loaded = await finish(await call('open_log',{log_id:row.log_id}));assert.equal(loaded.data.state,'succeeded');
  assert.equal(loaded.data.result.status,'complete');assert.equal(loaded.data.result.interval_seconds,10);
  await call('close_log',{log_ref:loaded.data.result.log_ref});
  const second = await start();assert.notEqual(first.recording_session_id,second.recording_session_id);
  const replay = await finish(await call('stop_recording',args));assert.deepEqual(replay.data,stopped.data);
  assert.deepEqual(fs.readFileSync(filename),bytes);
  const stale = await finish(await call('stop_recording',{recording_session_id:first.recording_session_id,operation_id:randomUUID()}));
  assert.equal(stale.data.state,'failed');assert.equal(stale.data.result.error.code,'STATE_CONFLICT');
  assert.equal((await call('get_recording')).data.recording_session_id,second.recording_session_id);
  const final = await finish(await call('stop_recording',{recording_session_id:second.recording_session_id,operation_id:randomUUID(),wait_ms:0}));
  assert.equal(final.data.state,'succeeded');
  assert.equal((await call('get_recording')).data.active,false);
  assert.equal((await call('get_status')).data.activity,'live');
  const result = {tools:tools.length,partial_frames:frames.length,complete_log_load:true,idempotent_retry:true,stale_stop_rejected:true,confirmation_required:false,calls};
  fs.writeFileSync(path.resolve(output),JSON.stringify(result,null,2)+'\n');
  console.log(JSON.stringify({...result,calls:calls.filter(c=>c.name==='stop_recording').map(c=>({ms:c.ms,state:c.result.data.state}))}));
} finally {
  // Clean up only sessions this harness started, if an assertion failed mid-run.
  try {
    const current = (await call('get_recording')).data;
    if(current.active && sessions.has(current.recording_session_id)) await finish(await call('stop_recording',{recording_session_id:current.recording_session_id,operation_id:randomUUID()}));
  } finally {await client.close();}
}
