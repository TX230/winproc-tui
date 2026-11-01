"""Run against an isolated TUI configuration; mutates tracking/profiles and records logs.
Arguments: exe, TUI PID, result JSON, optional ready-signalling .NET helper exe.
CONFIRM prompts require the local operator to confirm only the described test action.
"""
import datetime as dt
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import time
import uuid
spec=importlib.util.spec_from_file_location('h',Path(__file__).with_name('test-mcp-system.py'))
h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
absent='mcp-not-running-'+uuid.uuid4().hex[:8]+'.exe'
exe,pid,output=sys.argv[1:4];pid=int(pid);results=[];b=h.Bridge(exe);helper=None

def record(name,detail):
 results.append({'name':name,'status':'passed','detail':detail});Path(output).write_text(json.dumps(results,indent=2),encoding='utf-8');print(name+': passed',flush=True)
def data(name,args=None):return b.tool(name,args)['data']
def job(name,args):
 started=data(name,args);identity=started['job_id'];deadline=time.monotonic()+65
 while time.monotonic()<deadline:
  state=data('get_job',{'job_id':identity})
  if state['state']=='succeeded':return state['result']
  if state['state']=='failed':raise AssertionError(state)
  time.sleep(.1)
 raise TimeoutError(name)
def rev():return data('get_tracking')['revision']
def op(name,args,expected=None):
 args={'operation_id':str(uuid.uuid4()),**args}
 if name!='stop_recording':args.setdefault('expected_revision',rev())
 initial=data(name,args);identity=initial['operation_id'];prompted=False;deadline=time.monotonic()+65
 while time.monotonic()<deadline:
  result=data('get_operation',{'operation_id':identity})
  if result['state']=='pending_confirmation' and not prompted:
   print('CONFIRM '+name+' '+result.get('description',''),flush=True);prompted=True
  if result['state'] in ('succeeded','failed'):
   if expected:assert result['result']['error']['code']==expected,result
   else:assert result['state']=='succeeded',result
   return result
  time.sleep(.1)
 raise TimeoutError(name)
def profile(name):return next(p for p in data('list_profiles')['profiles'] if p['name']==name)
def profile_op(kind,profile_name,**extra):
 p=profile(profile_name);return op('update_profile',{'action':kind,'profile_id':p['profile_id'],'expected_profile_revision':p['revision'],**extra})
try:
 assert len(b.call('tools/list',{})['tools'])==26
 listed=data('list_processes',{'filter':{'pid':pid}});ref=listed['rows'][0]['process_ref'];data('release_capture',{'capture_id':listed['capture_id']})
 detail=job('get_process_info',{'process_ref':ref,'fields':['image','version']});rows=data('read_capture',{'capture_id':detail['capture_id']})['rows'];assert set(rows[0])=={'image','version'};data('release_capture',{'capture_id':detail['capture_id']});record('26 tools and process information',len(rows))
 first=op('update_tracking',{'action':'add','process_ref':ref});assert 'winproc-tui.exe' in first['result']['tracked_names']
 stale=rev();op('update_tracking',{'action':'add','process_name':absent});op('update_tracking',{'action':'add','process_name':'should-not-add.exe','expected_revision':stale},'STATE_CONFLICT')
 record('tracking and revision conflicts',data('get_tracking'))
 name='MCP system test '+uuid.uuid4().hex[:8];op('update_profile',{'action':'save_as','name':name});profile_op('save',name);profile_op('rename',name,name=name+' renamed')
 name+=' renamed';op('update_tracking',{'action':'remove','process_name':absent});profile_op('load',name);assert absent in data('get_tracking')['tracked_names'];record('profile save save_as rename load',name)
 appeared=job('wait_condition',{'condition':'process_appeared','process_name':'winproc-tui.exe','timeout_ms':5000});assert appeared['condition']=='process_appeared'
 threshold=job('wait_condition',{'condition':'metric_threshold','target':{'process_ref':ref},'metric_id':'private_bytes','comparison':'gt','threshold':'0','timeout_ms':5000});assert int(threshold['value'])>0;record('condition waits',threshold)
 since=dt.datetime.now(dt.timezone.utc).isoformat();started=op('start_recording',{'interval_seconds':2})['result'];assert started['active'];time.sleep(4)
 op('update_tracking',{'action':'remove','process_name':absent},'ACTIVITY_CONFLICT')
 session=started['recording_session_id'];b.close();b=h.Bridge(exe);time.sleep(2);current=data('get_recording');assert current['active'] and current['recording_session_id']==session and current['tracked_names']==started['tracked_names'],current;record('recording survives bridge reconnection',session);op('stop_recording',{'recording_session_id':session});assert not data('get_recording')['active'];record('recording lifecycle',session)
 listed_logs=job('list_logs',{});page=data('read_capture',{'capture_id':listed_logs['capture_id']});logs=page['rows'];data('release_capture',{'capture_id':listed_logs['capture_id']});chosen=next(r for r in logs if r['log_id']==started['output_log_id'])
 loaded=job('open_log',{'log_id':chosen['log_id']});logref=loaded['log_ref'];assert loaded['interval_seconds']==2
 logged=b.tool('list_processes',{'source':'log','log_ref':logref,'scope':'all'});assert logged['source']=='log';logrow=next(r for r in logged['data']['rows'] if r['pid']==pid);assert logrow['lifecycle']=='unknown';data('release_capture',{'capture_id':logged['data']['capture_id']})
 logprocess=logrow['process_ref'];data('get_metrics',{'source':'log','log_ref':logref,'targets':[{'process_ref':logprocess}],'metric_ids':['private_bytes']})
 b.tool('get_metrics',{'source':'live','targets':[{'process_ref':logprocess}],'metric_ids':['private_bytes']},error='REFERENCE_INVALID')
 history=b.tool('get_history',{'source':'log','log_ref':logref,'target':{'process_ref':logprocess},'metric_ids':['private_bytes'],'from':since,'to':dt.datetime.now(dt.timezone.utc).isoformat(),'mode':'raw'});assert history['data']['rows'];data('release_capture',{'capture_id':history['data']['capture_id']});data('close_log',{'log_ref':logref});b.tool('list_processes',{'source':'log','log_ref':logref},error='REFERENCE_INVALID');record('independent log analysis and source isolation',len(history['data']['rows']))
 replacement=op('start_recording',{'interval_seconds':10})['result'];assert replacement['recording_session_id']!=session
 op('stop_recording',{'recording_session_id':session},'STATE_CONFLICT');assert data('get_recording')['recording_session_id']==replacement['recording_session_id'];time.sleep(2)
 op('stop_recording',{'recording_session_id':replacement['recording_session_id']})
 partial_listing=job('list_logs',{});partial_rows=data('read_capture',{'capture_id':partial_listing['capture_id']})['rows'];data('release_capture',{'capture_id':partial_listing['capture_id']})
 assert any(r['log_id']==replacement['output_log_id'] for r in partial_rows)
 partial_log=job('open_log',{'log_id':replacement['output_log_id']});assert partial_log['interval_seconds']==10 and partial_log['status']=='complete',partial_log;data('close_log',{'log_ref':partial_log['log_ref']});record('stale stop rejected and partial recording window flushed',True)
 profile_op('delete',name);record('profile deletion',True)
 if len(sys.argv)>4:
  helper=subprocess.Popen([sys.argv[4]],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,creationflags=subprocess.CREATE_NO_WINDOW)
  assert helper.stdout.readline().strip()=='READY'
  target=None
  for _ in range(100):
   listing=data('list_processes',{'filter':{'pid':helper.pid}});data('release_capture',{'capture_id':listing['capture_id']})
   if listing['rows']:
    target=listing['rows'][0]['process_ref'];status=data('get_dotnet_collection',{'process_ref':target})
    if status['supported']:break
   time.sleep(.2)
  assert target and status['supported'],status
  op('set_dotnet_collection',{'process_ref':target,'enabled':True,'expected_revision':status['revision']})
  states=[]
  for _ in range(80):
   status=data('get_dotnet_collection',{'process_ref':target});states.append(status['state'])
   if status['state']=='collecting':break
   time.sleep(.2)
  assert any(s=='collecting' for s in states),states
  op('set_dotnet_collection',{'process_ref':target,'enabled':False,'expected_revision':status['revision']});assert not data('get_dotnet_collection',{'process_ref':target})['enabled'];record('explicit dotnet collection and revocation',states)
  wait=data('wait_condition',{'condition':'process_exited','target':{'process_ref':target},'timeout_ms':5000});time.sleep(.4);helper.terminate();helper.wait(timeout=5)
  for _ in range(50):
   state=data('get_job',{'job_id':wait['job_id']})
   if state['state']!='running':break
   time.sleep(.1)
  assert state['state']=='succeeded',state;record('verified process exit condition',state)
 op('update_tracking',{'action':'remove','process_name':absent})
 print('ALL WORKFLOWS PASSED',flush=True)
finally:
 if helper and helper.poll() is None:helper.terminate();helper.wait(timeout=5)
 b.close()
