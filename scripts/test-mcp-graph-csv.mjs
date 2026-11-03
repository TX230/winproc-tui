import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {randomUUID} from 'node:crypto';

// Python-free native MCP test. Use only an explicitly owned, initially empty test TUI.
// node scripts/test-mcp-graph-csv.mjs <exe> <tui-pid> <sdk-package-directory> <output-directory>
const [exe, pidText, sdkRoot, outputRoot] = process.argv.slice(2);
if (!exe || !pidText || !sdkRoot || !outputRoot) throw Error('Expected executable, owned TUI PID, MCP SDK directory and output directory');
const sdk = path.resolve(sdkRoot), output = path.resolve(outputRoot);
fs.mkdirSync(output, {recursive:true});
const {Client} = await import(pathToFileURL(path.join(sdk,'dist/esm/client/index.js')));
const {StdioClientTransport} = await import(pathToFileURL(path.join(sdk,'dist/esm/client/stdio.js')));
const connections = new Set(), calls = [], groups = [];
async function connect(raw=false) {
  const client = new Client({name:'winproc-graph-csv-test',version:'1'},{capabilities:{}});
  await client.connect(new StdioClientTransport({command:exe,args:['mcp-bridge',...(raw?['--raw']:[])],stderr:'pipe'}));
  connections.add(client);return client;
}
async function call(client, name, args={}) {
  const started = performance.now();
  const response = await client.callTool({name:'winproc_'+name,arguments:args});
  const value = response.structuredContent;
  assert.deepEqual(JSON.parse(response.content.find(c=>c.type==='text').text), value);
  calls.push({name,args,ms:performance.now()-started,response:value});
  assert(!response.isError,JSON.stringify(value));return value;
}
async function finish(client, value) {
  const deadline = performance.now()+10000;
  while (['pending','running'].includes(value.data.state)) {
    assert(performance.now()<deadline,JSON.stringify(value));
    const next = value.data.next_action;
    value = next
      ? await call(client,next.tool.replace('winproc_',''),next.arguments)
      : await call(client,'get_operation',{operation_id:value.data.operation_id,wait_ms:5000});
  }
  return value;
}
async function step(name, work) {
  const started = performance.now(), offset=calls.length;
  const evidence = await work();
  const group = {name,ms:performance.now()-started,calls:calls.length-offset,evidence};
  groups.push(group);console.log(JSON.stringify(group));return evidence;
}
const client = await connect();
try {
  const tools = (await client.listTools()).tools;
  assert.equal(tools.length,29);
  const status = await call(client,'get_status');assert.equal(status.data.activity,'live');assert.equal(status.data.stale,false);
  const recordingBefore = (await call(client,'get_recording')).data;
  assert.equal(recordingBefore.active,false);
  const initial = await call(client,'get_graphs',{process_name:'winproc-tui.exe'});
  assert.equal(initial.data.graphs.length,0,'Use an isolated empty test workspace; existing user Graphs must not be modified');
  assert(initial.data.processes.some(p=>p.pid===Number(pidText)&&p.identity_verified),'Owned test TUI PID is absent/unverified');
  await new Promise(resolve=>setTimeout(resolve,2100));
  const commit = await call(client,'get_metrics',{metric_ids:['committed_bytes']});
  const directory = path.join(output,'出力 テスト','nested');
  const combinedArgs = {metrics:[{metric_id:'cpu_average_percent'},{metric_id:'committed_bytes'}],output_directory:directory,filename:'system.csv'};
  const combined = await step('single_call_add_and_export',async()=>{
    const result = await finish(client,await call(client,'export_graphs',combinedArgs));
    assert.equal(result.data.state,'succeeded');assert.equal(result.data.result.workspace.graphs.length,2);
    assert(result.data.result.rows>=2);assert.equal(path.resolve(result.data.result.output_path),path.join(directory,'system.csv'));
    const csv = fs.readFileSync(result.data.result.output_path,'utf8');
    assert(csv.startsWith('timestamp,'));assert(!csv.includes('\r'));assert(!csv.startsWith('\uFEFF'));
    const rows = csv.trimEnd().split('\n').slice(1).map(line=>line.split(','));
    assert.equal(rows.length,result.data.result.rows);
    const observed = rows.find(row=>new Date(row[0]).getTime()===new Date(commit.captured_at).getTime());
    assert(observed,'CSV must retain the sampled metric timestamp');
    assert.equal(observed[2],commit.data.samples[0].values.committed_bytes);
    return {operation_id:result.data.operation_id,rows:rows.length,output_path:result.data.result.output_path,exact_commit_sample:true,workspace:result.data.result.workspace};
  });
  await step('process_graph_append_and_discovery',async()=>{
    const current = await call(client,'get_graphs',{process_name:'winproc-tui.exe'});
    const target = current.data.processes.find(p=>p.pid===Number(pidText));assert(target);
    const result = await finish(client,await call(client,'add_graphs',{metrics:[{metric_id:'private_bytes',process_ref:target.process_ref},{metric_id:'private_bytes',process_ref:target.process_ref}],expected_revision:current.data.revision,operation_id:randomUUID()}));
    assert.equal(result.data.state,'succeeded');assert.equal(result.data.result.graphs.length,3);
    assert.deepEqual(result.data.result.graphs.slice(0,2),combined.workspace.graphs);
    assert.equal(result.data.result.active_graph_id,combined.workspace.active_graph_id);
    return {slots:3,duplicate_ignored:true,existing_graph_ids_preserved:true};
  });
  await step('retry_and_collision',async()=>{
    const before = fs.readFileSync(combined.output_path);
    const replay = await finish(client,await call(client,'export_graphs',{...combinedArgs,operation_id:combined.operation_id}));
    assert.equal(replay.data.state,'succeeded');assert.deepEqual(replay.data.result.workspace,combined.workspace);
    const collision = await finish(client,await call(client,'export_graphs',{output_directory:directory,filename:'system.csv'}));
    assert.equal(collision.data.state,'failed');assert.equal(collision.data.result.error.code,'EXPORT_FAILED');
    assert.deepEqual(fs.readFileSync(combined.output_path),before);
    const current = await call(client,'get_graphs');assert.equal(current.data.graphs.length,3);
    return {replay_did_not_repeat:true,collision:'EXPORT_FAILED',existing_file_preserved:true};
  });
  await step('raw_export_and_reconnected_outcome',async()=>{
    const raw = await connect(true);
    const start = await call(raw,'export_graphs',{output_directory:directory,filename:'all.csv'});
    const operation_id = start.data.operation_id;assert(operation_id);
    let accepted=start;
    while(accepted.data.state==='pending') accepted=await call(raw,'get_operation',{operation_id,wait_ms:5000});
    assert(['running','succeeded'].includes(accepted.data.state));
    await raw.close();connections.delete(raw);
    const result = await finish(client,await call(client,'get_operation',{operation_id}));
    assert.equal(result.data.state,'succeeded');assert.equal(result.data.result.workspace.graphs.length,3);
    assert.equal(fs.readFileSync(result.data.result.output_path,'utf8').trimEnd().split('\n').length-1,result.data.result.rows);
    return {state_before_disconnect:accepted.data.state,retained_after_disconnect:true,rows:result.data.result.rows};
  });
  const recordingAfter = (await call(client,'get_recording')).data;assert.deepEqual(recordingAfter,recordingBefore);
  const final = (await call(client,'get_status')).data;assert.equal(final.activity,'live');assert.equal(final.stale,false);
  fs.writeFileSync(path.join(output,'native-results.json'),JSON.stringify({tools:tools.length,groups,calls,recording_unchanged:true,final},null,2)+'\n');
} finally {
  for (const connection of connections) await connection.close();
}
