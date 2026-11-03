// Run a caller-authored MCP workflow in one connection, without starting another AI agent.
// Usage: node scripts/run-mcp-workflow.mjs <exe> <sdk-package-directory> <workflow.mjs> <output.json>
// Workflow module: export default async ({ call, tools }) => result;
// All capture/process/job references are scoped to this connection and expire on exit.
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {writeFile} from 'node:fs/promises';
import {performance} from 'node:perf_hooks';

const [exe,sdkRoot,workflow,output]=process.argv.slice(2);
if(!exe||!sdkRoot||!workflow||!output)throw Error('Expected executable, installed MCP SDK package directory, caller-authored workflow module and output path');
const {Client}=await import(pathToFileURL(resolve(sdkRoot,'dist/esm/client/index.js')));
const {StdioClientTransport}=await import(pathToFileURL(resolve(sdkRoot,'dist/esm/client/stdio.js')));
const run=(await import(pathToFileURL(resolve(workflow)))).default;
if(typeof run!=='function')throw Error('Workflow must export a default function');
const client=new Client({name:'winproc-workflow',version:'1'},{capabilities:{}});
const started=performance.now(),calls=[];
let listed;
try{
  await client.connect(new StdioClientTransport({command:resolve(exe),args:['mcp-bridge'],stderr:'pipe'}));
  const catalog=await client.listTools();listed=performance.now();
  const tools=new Set(catalog.tools.map(tool=>tool.name));
  const call=async(name,args={})=>{
    if(!tools.has(name))throw Error('Unknown discovered tool: '+name);
    const before=performance.now();
    const response=await client.callTool({name,arguments:args});
    calls.push({tool:name,start_ms:before-started,duration_ms:performance.now()-before,bytes:Buffer.byteLength(JSON.stringify(response)),is_error:response.isError??false});
    if(response.isError)throw Error(JSON.stringify(response.structuredContent??response.content));
    return response.structuredContent;
  };
  const result=await run({call,tools:catalog.tools});
  const evidence={connect_and_discovery_ms:listed-started,discovery_to_first_call_ms:calls[0]?calls[0].start_ms-(listed-started):null,execution_ms:performance.now()-started,calls,result};
  await writeFile(resolve(output),JSON.stringify(evidence,null,2)+'\n');
  console.log(JSON.stringify({connect_and_discovery_ms:evidence.connect_and_discovery_ms,discovery_to_first_call_ms:evidence.discovery_to_first_call_ms,execution_ms:evidence.execution_ms,calls:calls.length,output:resolve(output)}));
}finally{await client.close();}
