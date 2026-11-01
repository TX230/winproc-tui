"""Interactive Settings lifecycle test.
Arguments: executable, PID, coordination-file path, output JSON path, optional --recording.
With --recording, use an isolated configured recording directory and nonempty
Tracking List; the test starts Recording and prompts for local confirmed stop.
After each printed prompt, toggle Settings in the TUI and write disabled/enabled
to the coordination file. No terminal key events are synthesized by this script.
"""
import datetime as dt
import importlib.util
import json
from pathlib import Path
import sys
import time
import uuid
spec=importlib.util.spec_from_file_location('h',Path(__file__).with_name('test-mcp-system.py'))
h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
exe,pid,marker,output=sys.argv[1:5];marker=Path(marker)
marker.write_text('initial',encoding='utf-8')
def wait(state):
    deadline=time.monotonic()+120
    while time.monotonic()<deadline:
        if marker.read_text(encoding='utf-8').strip()==state:return
        time.sleep(.1)
    raise TimeoutError('Settings coordination: '+state)
b=h.Bridge(exe)
try:
    recording=None
    if '--recording' in sys.argv:
        revision=b.tool('get_tracking')['data']['revision']
        operation=str(uuid.uuid4())
        b.tool('start_recording',{'operation_id':operation,'expected_revision':revision,'interval_seconds':2})
        for _ in range(100):
            state=b.tool('get_operation',{'operation_id':operation})['data']
            if state['state']=='succeeded':recording=state['result'];break
            assert state['state']=='pending',state
            time.sleep(.1)
        assert recording, 'Recording did not start'
    initial=b.tool('list_processes' ,{'filter':{'pid':int(pid)}})
    old=initial['data']['rows'][0]['process_ref']
    before=initial['captured_at']
    print('READY: disable MCP connections, then signal disabled',flush=True)
    wait('disabled')
    reply=b.call('tools/call',{'name':'winproc_get_status','arguments':{}})
    assert reply['isError'],reply
    assert reply['structuredContent']['error']['code'] in ['OUTCOME_UNKNOWN','BACKEND_UNAVAILABLE'],reply
    other=h.Bridge(exe)
    try:other.tool('get_status',error='BACKEND_UNAVAILABLE')
    finally:other.close()
    print('DISABLED VERIFIED: enable MCP connections, then signal enabled',flush=True)
    wait('enabled')
    status=b.tool('get_status')
    assert status['captured_at']>before and not status['data']['stale'],status
    b.tool('get_metrics',{'source':'live','targets':[{'process_ref':old}],'metric_ids':['private_bytes']},error='REFERENCE_INVALID')
    listed=b.tool('list_processes',{'filter':{'pid':int(pid)}})
    ref=listed['data']['rows'][0]['process_ref']
    history=b.tool('get_history',{'source':'live','target':{'process_ref':ref},'metric_ids':['private_bytes'],'from':before,'to':status['captured_at'],'mode':'raw','limit':100})
    times=[dt.datetime.fromisoformat(row['captured_at']) for row in history['data']['rows']]
    gaps=[(last-first).total_seconds() for first,last in zip(times,times[1:])]
    assert len(times)>=2 and max(gaps)<3,(len(times),gaps)
    result={'existing_connection_closed':True,'new_connection_refused':True,'reenable_reconnect':True,'old_reference_rejected':True,'sampling_continued':True,'samples':len(times),'max_sample_gap_seconds':max(gaps)}
    if recording:
        current=b.tool('get_recording')['data']
        assert current['active'] and current['recording_session_id']==recording['recording_session_id'],current
        assert current['tracked_names']==recording['tracked_names'],current
        result['recording_survives_disable_enable']=True
        operation=str(uuid.uuid4())
        b.tool('stop_recording',{'operation_id':operation,'recording_session_id':recording['recording_session_id']})
        print('CONFIRM STOP: y in the local TUI',flush=True)
        for _ in range(600):
            state=b.tool('get_operation',{'operation_id':operation})['data']
            if state['state']=='succeeded':break
            assert state['state'] in ('pending','pending_confirmation'),state
            time.sleep(.1)
        assert state['state']=='succeeded',state
    Path(output).write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result),flush=True)
finally:b.close()
