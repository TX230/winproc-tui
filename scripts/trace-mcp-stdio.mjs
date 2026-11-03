// Transparent stdio forwarding. Records timing/size metadata, never captured values or arguments.
import {spawn} from 'node:child_process';import fs from 'node:fs';
const [exe,log]=process.argv.slice(2),pending=new Map();
if(!exe||!log)throw Error('Expected executable and timing-log path');
const out=fs.openSync(log,'w');
function record(event,fields={}){fs.writeSync(out,JSON.stringify({event,epoch_ms:performance.timeOrigin+performance.now(),ns:process.hrtime.bigint().toString(),...fields})+'\n');}
const child=spawn(exe,['mcp-bridge'],{stdio:['pipe','pipe','pipe'],windowsHide:true});
record('bridge_spawn');child.stderr.pipe(process.stderr);
function lines(stream,handle){let buf='';stream.setEncoding('utf8');stream.on('data',s=>{buf+=s;let end;while((end=buf.indexOf('\n'))>=0){const line=buf.slice(0,end);buf=buf.slice(end+1);handle(line);}});}
lines(process.stdin,line=>{let msg;try{msg=JSON.parse(line);}catch{}const method=msg?.method??'unknown';if(msg?.id!=null)pending.set(String(msg.id),method);record('client_request',{method,id:msg?.id,tool:msg?.params?.name,bytes:Buffer.byteLength(line)});child.stdin.write(line+'\n');});
lines(child.stdout,line=>{
 let msg;try{msg=JSON.parse(line);}catch{}
 const method=pending.get(String(msg?.id));pending.delete(String(msg?.id));
 const fields={method,id:msg?.id,bytes_original:Buffer.byteLength(line)};
 if(method==='tools/list'&&msg?.result?.tools)fields.tools=msg.result.tools.length;
 record('server_response',fields);
 process.stdout.write(line+'\n',()=>record('response_flushed',{method,id:msg?.id,bytes_sent:Buffer.byteLength(line)}));
});
process.stdin.on('end',()=>child.stdin.end());child.on('exit',(code,signal)=>{record('bridge_exit',{code,signal});process.exitCode=code??0;});
process.on('SIGTERM',()=>child.kill());process.on('SIGINT',()=>child.kill());
