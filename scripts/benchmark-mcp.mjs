import {resolve} from 'node:path';import {pathToFileURL} from 'node:url';import {writeFile} from 'node:fs/promises';import {performance} from 'node:perf_hooks';
// Usage: node scripts/benchmark-mcp.mjs <exe> <tui-pid> <sdk-package-dir> <output.json> [async|completion]
// Read-only: no tracking/profile/recording/scheduling mutations or process termination.
// The application must be running. A configured recording root is optional; its absence is reported.
const [exe,pid,sdkRoot,output,mode='completion']=process.argv.slice(2);
if(!exe||!pid||!sdkRoot||!output||!['async','completion'].includes(mode))throw Error('Expected executable, TUI PID, installed MCP SDK package directory, output path and optional mode');
const sdk=resolve(sdkRoot);
const {Client}=await import(pathToFileURL(resolve(sdk,'dist/esm/client/index.js')));const {StdioClientTransport}=await import(pathToFileURL(resolve(sdk,'dist/esm/client/stdio.js')));
const fused=mode==='completion',calls=[],groups=[];
const make=async()=>{const c=new Client({name:'winproc-perf-categories',version:'1'},{capabilities:{}});await c.connect(new StdioClientTransport({command:resolve(exe),args:['mcp-bridge','--raw'],stderr:'pipe'}));return c;};
const client=await make();const captures=new Set();
async function call(name,args={},allowError=false,c=client){const t=performance.now();const r=await c.callTool({name:'winproc_'+name,arguments:args});calls.push({name,ms:performance.now()-t,bytes:Buffer.byteLength(JSON.stringify(r)),error:r.isError??false});if(r.isError&&!allowError)throw Error(JSON.stringify(r));return r.structuredContent;}
async function group(name,work){const t=performance.now(),offset=calls.length;const evidence=await work();const g={name,ms:performance.now()-t,calls:calls.length-offset,evidence};groups.push(g);console.log(JSON.stringify(g));return evidence;}
async function job(name,args={},read={limit:5}){
 let r=await call(name,{...args,...(fused?{wait_ms:5000,...(read?{read}:{})}:{})});
 while(r.data.state==='running'){await new Promise(r=>setTimeout(r,20));r=await call('get_job',{job_id:r.data.job_id,...(fused?{wait_ms:5000,...(read?{read}:{})}:{})});}
 if(r.data.state!=='succeeded')throw Error(JSON.stringify(r));
 if(r.data.result?.capture_id){captures.add(r.data.result.capture_id);const p=r.data.result_page??await call('read_capture',{capture_id:r.data.result.capture_id,...read});if(p.error)throw Error(JSON.stringify(p));captures.add(p.data.capture_id);return {state:r.data.state,status:p.status,rows:p.data.rows.length,listed:p.data.coverage.listed,scan_started:r.data.result.capture_started_at,scan_finished:r.data.result.capture_finished_at,result:r.data.result,page:p.data};}
 return r.data;
}
try{
 const catalog=await client.listTools();
 await group('cached_getters',async()=>{for(let i=0;i<10;i++)for(const n of ['get_status','get_system_info','list_metrics','get_tracking','list_profiles','get_recording'])await call(n);return {tools:catalog.tools.length};});
 const listed=await call('list_processes',{filter:{pid:Number(pid)}});captures.add(listed.data.capture_id);const ref=listed.data.rows[0].process_ref;
 await group('metrics_and_history',async()=>{await call('get_metrics',{source:'live',targets:[{system:true}],metric_ids:['cpu_average_percent']});for(const mode of ['summary','raw']){const r=await call('get_history',{source:'live',target:{system:true},metric_ids:['cpu_average_percent'],from:new Date(Date.now()-5000).toISOString(),to:new Date().toISOString(),mode,limit:5});if(r.data.capture_id)captures.add(r.data.capture_id);}await call('get_dotnet_collection',{process_ref:ref});return 'passed';});
 for(const kind of ['network','threads','scheduling','open_files','modules']){await group(kind,async()=>{const r=await job('start_capture',{kind,process_ref:ref});for(const id of captures)await call('release_capture',{capture_id:id});captures.clear();return {state:r.state,status:r.status,listed:r.listed};});}
 await group('process_info',async()=>{const r=await job('get_process_info',{process_ref:ref});return {state:r.state,status:r.status,listed:r.listed};});
 const logs=await group('saved_logs',async()=>{const r=await call('list_logs',fused?{wait_ms:5000,read:{limit:5}}:{},true);if(r.error)return {unavailable:r.error.code};let status=r;while(status.data.state==='running'){await new Promise(r=>setTimeout(r,20));status=await call('get_job',{job_id:status.data.job_id,...(fused?{wait_ms:5000,read:{limit:5}}:{})});}if(status.data.state!=='succeeded')return status.data;captures.add(status.data.result.capture_id);const page=status.data.result_page??await call('read_capture',{capture_id:status.data.result.capture_id,limit:5});captures.add(page.data.capture_id);const first=page.data.rows[0];if(!first)return {listed:0};const load=await job('open_log',{log_id:first.log_id},null);await call('get_metrics',{source:'log',log_ref:load.result.log_ref,targets:[{system:true}],metric_ids:['cpu_average_percent']});await call('close_log',{log_ref:load.result.log_ref});return {listed:page.data.coverage.listed,loaded:true};});
 await group('condition_wait',async()=>{const r=await job('wait_condition',{condition:'process_appeared',process_name:'winproc-tui.exe',timeout_ms:3000},null);return r.state;});
 await group('operation_outcome',async()=>{const r=await call('get_operation',{operation_id:'benchmark-unknown-operation',...(fused?{wait_ms:5000}:{})},true);return {expected_error:r.error?.code};});
 if(fused){const second=await make();try{await group('concurrent_client_during_wait',async()=>{const pending=call('wait_condition',{condition:'process_appeared',process_name:'winproc-perf-never-appears-205.exe',timeout_ms:1500,wait_ms:5000},false,client);await new Promise(r=>setTimeout(r,75));const t=performance.now();await call('get_status',{},false,second);const other=performance.now()-t;const r=await pending;if(other>500)throw Error('dispatcher blocked '+other);if(r.data.state!=='failed'||r.data.result.error?.code!=='TIMEOUT')throw Error(JSON.stringify(r));return {other_client_ms:other,wait_state:r.data.state};});}finally{await second.close();}}
 const result={mode,groups,calls};await writeFile(resolve(output),JSON.stringify(result,null,2)+'\n');
}finally{for(const id of captures)await call('release_capture',{capture_id:id},true).catch(()=>{});await client.close();}
