"""Additional MCP adversarial system checks. Use an isolated config with a saved log.
Arguments: executable, output JSON. Does not accept local confirmation dialogs.
"""
import importlib.util,json,sys,time,uuid
from pathlib import Path
spec=importlib.util.spec_from_file_location('h',Path(__file__).with_name('test-mcp-system.py'));h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
exe,out=sys.argv[1:3];results=[]
def check(name,fn):
 try: detail=fn(); row={'name':name,'status':'passed','detail':detail}
 except Exception as e: row={'name':name,'status':'failed','detail':str(e)}
 results.append(row);Path(out).write_text(json.dumps(results,indent=2),encoding='utf-8');print(json.dumps(row),flush=True)
def data(b,n,a=None):return b.tool(n,a)['data']
def finished(b,j):
 end=time.monotonic()+70
 while time.monotonic()<end:
  r=data(b,'get_job',{'job_id':j})
  if r['state']!='running':return r
  time.sleep(.05)
 raise TimeoutError(j)
def operation(b,a):
 data(b,'update_profile',a)
 end=time.monotonic()+5
 while time.monotonic()<end:
  r=data(b,'get_operation',{'operation_id':a['operation_id']})
  if r['state']!='pending':return r
  time.sleep(.05)
 raise TimeoutError(a)
def replay_and_timeout():
 b=h.Bridge(exe);c=None
 try:
  name='Review '+uuid.uuid4().hex[:8]
  a={'action':'save_as','name':name,'operation_id':str(uuid.uuid4()),'expected_revision':data(b,'get_tracking')['revision']}
  first=operation(b,a);assert first['state']=='succeeded',first
  b.close();b=h.Bridge(exe)
  replay=operation(b,a);assert replay==first,(first,replay)
  p=next(p for p in data(b,'list_profiles')['profiles'] if p['name']==name)
  delete={'action':'delete','profile_id':p['profile_id'],'expected_profile_revision':p['revision'],'expected_revision':p['revision'],'operation_id':str(uuid.uuid4())}
  pending=operation(b,delete);assert pending['state']=='pending_confirmation',pending
  b.close();b=h.Bridge(exe);time.sleep(.3)
  cancelled=data(b,'get_operation',{'operation_id':delete['operation_id']});assert cancelled['result']['error']['code']=='CANCELLED',cancelled
  row={'name':'operation replay and disconnect cancellation','status':'passed'};results.append(row);print(json.dumps(row),flush=True)
  delete['operation_id']=str(uuid.uuid4());pending=operation(b,delete);assert pending['state']=='pending_confirmation',pending
  deadline=time.monotonic()+65
  while time.monotonic()<deadline:
   state=data(b,'get_operation',{'operation_id':delete['operation_id']})
   if state['state']=='failed':break
   time.sleep(.25)
  assert any(p['name']==name for p in data(b,'list_profiles')['profiles'])
  assert state['result']['error']['code']=='TIMEOUT',state
  return {'replay':True,'disconnect_cancel':True,'timeout':True}
 finally:b.close()
def mixed_capacity():
 b=h.Bridge(exe)
 try:
  listing=finished(b,data(b,'list_logs')['job_id'])['result'];rows=data(b,'read_capture',{'capture_id':listing['capture_id']})['rows'];data(b,'release_capture',{'capture_id':listing['capture_id']})
  assert rows,'Prerequisite: create one recording in isolated log directory'
  loaded=finished(b,data(b,'open_log',{'log_id':rows[0]['log_id']})['job_id']);assert loaded['state']=='succeeded',loaded
  logref=loaded['result']['log_ref']
  job=data(b,'start_capture',{'kind':'file_users'})['job_id']
  ids=[]
  for _ in range(7):
   r=b.call('tools/call',{'name':'winproc_list_processes','arguments':{'source':'log','log_ref':logref}})['structuredContent']
   if 'error' in r:break
   ids.append(r['data']['capture_id'])
  state=finished(b,job)
  native=state.get('result',{}).get('capture_id')
  if native: ids.append(native)
  assert 1+len(ids)<=8,{'retained_log_count':1,'returned_capture_count':len(ids),'job':state}
  return {'retained_objects':1+len(ids)}
 finally:b.close()
def full_capture_read():
 b=h.Bridge(exe)
 try:
  captures=[data(b,'list_processes',{'filter':{'pid':0}})['capture_id'] for _ in range(8)]
  result=data(b,'read_capture',{'capture_id':captures[0]})
  assert result['capture_id']==captures[0],result
  return 'Existing frozen capture remains readable at capacity'
 finally:b.close()
def malformed_logs():
 root=Path(exe).resolve().parent/'logs'
 original=next(root.glob('winproc-mcp-*.log')).read_bytes().splitlines(keepends=True)
 prefix='review-'+uuid.uuid4().hex
 fixtures={
  'truncated':(b''.join(original[:-1])+b'{','succeeded',None),
  'malformed':(b''.join(original)+b'{\n','failed','INVALID_ARGUMENT'),
  'oversized':(b''.join(original)+b' '*(4*1024*1024+1)+b'\n','failed','RESULT_TOO_LARGE')}
 paths=[];b=h.Bridge(exe)
 try:
  for kind,(payload,_,_) in fixtures.items():
   path=root/(prefix+'-'+kind+'.log');path.write_bytes(payload);paths.append(path)
  listing=finished(b,data(b,'list_logs',{'filter':prefix})['job_id'])['result']
  rows=data(b,'read_capture',{'capture_id':listing['capture_id']})['rows'];data(b,'release_capture',{'capture_id':listing['capture_id']})
  for kind,(_,state,code) in fixtures.items():
   row=next(r for r in rows if r['name']==prefix+'-'+kind+'.log')
   result=finished(b,data(b,'open_log',{'log_id':row['log_id']})['job_id']);assert result['state']==state,result
   if code:assert result['result']['error']['code']==code,result
   else:
    assert result['result']['status']=='partial',result
    data(b,'close_log',{'log_ref':result['result']['log_ref']})
  return 'Interrupted tail preserved; malformed complete line and oversized line rejected'
 finally:
  b.close()
  for path in paths:path.unlink(missing_ok=True)
check('saved-log truncated, malformed, and oversized records',malformed_logs)
check('unmodified frozen capture read at capacity',full_capture_read)
check('mixed saved-log and asynchronous capture budget',mixed_capacity)
if '--quick' not in sys.argv: check('operation reconnect replay, cancellation, and timeout',replay_and_timeout)
sys.exit(int(any(r['status']=='failed' for r in results)))
