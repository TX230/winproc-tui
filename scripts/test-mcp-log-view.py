"""Exercise source isolation across interactive Log view. Arguments: exe, result JSON.
When prompted, open a saved recording in the TUI (Ctrl+L then Enter), then return
with Ctrl+B. Uses an existing recording in the isolated configured log directory.
"""
import importlib.util,json,sys,time
from pathlib import Path
spec=importlib.util.spec_from_file_location('h',Path(__file__).with_name('test-mcp-system.py'));h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
b=h.Bridge(sys.argv[1]);results={}
def data(n,a=None):return b.tool(n,a)['data']
def job(n,a):
 j=data(n,a)['job_id']
 for _ in range(600):
  r=data('get_job',{'job_id':j})
  if r['state']=='succeeded':return r['result']
  assert r['state']=='running',r;time.sleep(.1)
 raise TimeoutError(n)
try:
 listing=job('list_logs',{});rows=data('read_capture',{'capture_id':listing['capture_id']})['rows'];data('release_capture',{'capture_id':listing['capture_id']})
 log=job('open_log',{'log_id':rows[0]['log_id']})['log_ref']
 live=data('list_processes',{'limit':1});capture=live['capture_id'];wait=data('wait_condition',{'condition':'process_appeared','process_name':'mcp-never-for-log-view.exe','timeout_ms':60000})['job_id']
 print('ENTER LOG VIEW: Ctrl+L, wait for list, Enter',flush=True)
 for _ in range(500):
  if data('get_status')['activity']=='log_view':break
  time.sleep(.1)
 else:raise TimeoutError('log view')
 b.tool('list_processes',{},error='ACTIVITY_CONFLICT');b.tool('read_capture',{'capture_id':capture},error='REFERENCE_INVALID')
 state=data('get_job',{'job_id':wait});assert state['state']=='failed',state
 logged=b.tool('list_processes',{'source':'log','log_ref':log});assert logged['source']=='log';results['live_rejected_and_saved_log_survives']=True
 print('RETURN LIVE: Ctrl+B',flush=True)
 for _ in range(500):
  if data('get_status')['activity']=='live':break
  time.sleep(.1)
 else:raise TimeoutError('return live')
 assert b.tool('list_processes',{'source':'log','log_ref':log})['source']=='log';data('close_log',{'log_ref':log});results['return_live_preserves_independent_log']=True
 Path(sys.argv[2]).write_text(json.dumps(results,indent=2),encoding='utf-8');print('LOG VIEW PASSED',flush=True)
finally:b.close()
