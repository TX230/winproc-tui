// Arguments: executable, TUI PID, installed SDK package directory, result JSON path.
import {resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
import {readFile,writeFile} from 'node:fs/promises';
const [exe,pid,sdkRoot,output]=process.argv.slice(2);
const {Client}=await import(pathToFileURL(resolve(sdkRoot,'dist/esm/client/index.js')));
const {StdioClientTransport}=await import(pathToFileURL(resolve(sdkRoot,'dist/esm/client/stdio.js')));
const pkg=JSON.parse(await readFile(resolve(sdkRoot,'package.json'),'utf8'));
const client=new Client({name:'winproc-system-typescript',version:'1'},{capabilities:{}});
let protocol;
const originalRequest=client.request.bind(client);
client.request=async (...args)=>{const result=await originalRequest(...args);if(args[0].method==='initialize')protocol=result.protocolVersion;return result;};
const transport=new StdioClientTransport({command:resolve(exe),args:['mcp-bridge']});
function assert(condition,detail){if(!condition)throw new Error(JSON.stringify(detail));}
try{
  await client.connect(transport);
  const tools=await client.listTools();
  async function call(name,args={}){
    const response=await client.callTool({name:'winproc_'+name,arguments:args});
    assert(!response.isError,response);
    assert(JSON.stringify(JSON.parse(response.content[0].text))===JSON.stringify(response.structuredContent),'structured/text mismatch');
    return response.structuredContent;
  }
  const processes=await call('list_processes',{filter:{pid:Number(pid)}});
  const reference=processes.data.rows[0].process_ref;
  const metrics=await call('get_metrics',{source:'live',targets:[{process_ref:reference}],metric_ids:['private_bytes']});
  assert(typeof metrics.data.samples[0].values.private_bytes==='string',metrics);
  const job=(await call('start_capture',{kind:'open_files',process_ref:reference})).data.job_id;
  let status;
  for(let index=0;index<600;index++){
    status=(await call('get_job',{job_id:job})).data;
    if(status.state!=='running')break;
    await new Promise(resolve=>setTimeout(resolve,100));
  }
  assert(status.state==='succeeded',status);
  const report=status.result;
  const page=await call('read_capture',{capture_id:report.capture_id,limit:5});
  assert(page.status===report.status,page);
  for(const capture of new Set([report.capture_id,page.data.capture_id,processes.data.capture_id]))await call('release_capture',{capture_id:capture});
  const result={client:'TypeScript MCP SDK',version:pkg.version,protocol,tools:tools.tools.length,process_metrics_files:'passed',file_rows:page.data.coverage.listed,partial:page.status==='partial'};
  await writeFile(output,JSON.stringify(result,null,2)+'\n');
  console.log(JSON.stringify(result));
}finally{await client.close();}
