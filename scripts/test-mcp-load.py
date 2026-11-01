"""Compare live sample cadence under idle, request load, and locally disabled MCP.
Arguments: executable, coordination marker, output JSON. Each phase lasts 15 seconds.
Toggle Settings when prompted, then write disabled/enabled to the marker.
"""
import datetime as dt,importlib.util,json,threading,time,sys
from pathlib import Path
spec=importlib.util.spec_from_file_location('h',Path(__file__).with_name('test-mcp-system.py'));h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
exe,marker,output=sys.argv[1:];marker=Path(marker);marker.write_text('initial');phases=[];counts=[];errors=[]
def now():return dt.datetime.now(dt.timezone.utc)
def wait_marker(state):
 deadline=time.monotonic()+60
 while time.monotonic()<deadline:
  if marker.read_text().strip()==state:return
  time.sleep(.1)
 raise TimeoutError(state)
b=h.Bridge(exe)
try:
 metric=b.tool('list_metrics',{'target_kind':'system'})['data']['metrics'][0]['id']
 start=now();time.sleep(15);phases.append(('idle_enabled',start,now()))
 start=now();deadline=time.monotonic()+15
 def load():
  client=h.Bridge(exe);count=0
  try:
   while time.monotonic()<deadline:
    client.tool('get_metrics',{'source':'live','targets':[{'system':True}],'metric_ids':[metric]});count+=1
  except Exception as e:errors.append(str(e))
  finally:client.close();counts.append(count)
 workers=[threading.Thread(target=load) for _ in range(3)]
 for worker in workers:worker.start()
 for worker in workers:worker.join(timeout=30)
 assert not any(w.is_alive() for w in workers)
 phases.append(('three_client_load',start,now()));b.close()
 print('DISABLE MCP, then write disabled to marker',flush=True);wait_marker('disabled');start=now();time.sleep(15);phases.append(('disabled',start,now()))
 print('ENABLE MCP, then write enabled to marker',flush=True);wait_marker('enabled');b=h.Bridge(exe)
 result=b.tool('get_history',{'source':'live','target':{'system':True},'metric_ids':[metric],'from':phases[0][1].isoformat(),'to':now().isoformat(),'mode':'raw','limit':500})['data'];rows=result['rows']
 timestamps=[dt.datetime.fromisoformat(r['captured_at']) for r in rows]
 evidence={'request_counts':counts,'errors':errors,'phases':[]}
 for name,start,end in phases:
  times=[t for t in timestamps if start<=t<=end];gaps=[(r-l).total_seconds() for l,r in zip(times,times[1:])]
  evidence['phases'].append({'name':name,'samples':len(times),'max_gap_seconds':max(gaps,default=None),'mean_gap_seconds':sum(gaps)/len(gaps) if gaps else None})
  assert len(times)>=12 and max(gaps)<3,evidence
 assert not errors,evidence
 Path(output).write_text(json.dumps(evidence,indent=2),encoding='utf-8');print(json.dumps(evidence),flush=True)
finally:b.close()
