"""Windows live-backend MCP system checks; all generated evidence stays local.

Start the TUI separately, then pass its executable and PID. The harness creates
and stops only its own bridge/helper processes. It does not mutate tracking,
profiles, recording, or other user application state.
"""
import argparse
import ctypes
from ctypes import wintypes
import datetime as dt
import json
import os
from pathlib import Path
import queue
import shutil
import uuid
import subprocess
import sys
import tempfile
import threading
import time


class Bridge:
    def __init__(self, exe, *, env=None, timeout=15):
        self.timeout = timeout
        self.process = subprocess.Popen([str(exe), 'mcp-bridge'], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8',
            creationflags=subprocess.CREATE_NO_WINDOW, env=env)
        self.messages = queue.Queue()
        self.sequence = 0
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()
        result = self.call('initialize', {'protocolVersion':'2025-11-25', 'capabilities':{},
            'clientInfo':{'name':'winproc-system-harness','version':'1'}})
        assert result['protocolVersion'] == '2025-11-25', result
        self.notify('notifications/initialized', {})

    def _read(self):
        try:
            for line in self.process.stdout:
                self.messages.put(json.loads(line))
        except Exception as error:
            self.messages.put(error)

    def notify(self, method, params):
        self.process.stdin.write(json.dumps({'jsonrpc':'2.0','method':method,'params':params})+'\n')
        self.process.stdin.flush()

    def call(self, method, params):
        self.sequence += 1
        identity = self.sequence
        self.process.stdin.write(json.dumps({'jsonrpc':'2.0','id':identity,'method':method,'params':params})+'\n')
        self.process.stdin.flush()
        deadline = time.monotonic()+self.timeout
        while time.monotonic() < deadline:
            reply = self.messages.get(timeout=max(.1, deadline-time.monotonic()))
            if isinstance(reply, Exception): raise reply
            if reply.get('id') != identity: continue
            if 'error' in reply: raise AssertionError(reply['error'])
            return reply['result']
        raise TimeoutError(method)

    def tool(self, name, args=None, error=None):
        result = self.call('tools/call', {'name':'winproc_'+name, 'arguments':args or {}})
        structured = result['structuredContent']
        assert json.loads(result['content'][0]['text']) == structured, 'text/structured mismatch'
        if error:
            assert result.get('isError') and structured['error']['code'] == error, result
        else:
            assert not result.get('isError'), structured
        return structured

    def close(self):
        if self.process.stdin and not self.process.stdin.closed: self.process.stdin.close()
        # The SDK drains in-flight replies for up to five seconds after EOF.
        try: self.process.wait(timeout=8)
        except subprocess.TimeoutExpired: self.process.terminate(); self.process.wait(timeout=5)
        self.reader.join(timeout=1)


def monitor(pid, stop, rows):
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.OpenProcess.argtypes = [wintypes.DWORD,wintypes.BOOL,wintypes.DWORD]
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    class Memory(ctypes.Structure):
        _fields_ = [('cb',wintypes.DWORD),('PageFaultCount',wintypes.DWORD)] + [(name,ctypes.c_size_t) for name in
            ['PeakWorkingSetSize','WorkingSetSize','QuotaPeakPagedPoolUsage','QuotaPagedPoolUsage',
             'QuotaPeakNonPagedPoolUsage','QuotaNonPagedPoolUsage','PagefileUsage','PeakPagefileUsage','PrivateUsage']]
    kernel.K32GetProcessMemoryInfo.argtypes = [wintypes.HANDLE,ctypes.POINTER(Memory),wintypes.DWORD]
    kernel.GetProcessHandleCount.argtypes = [wintypes.HANDLE,ctypes.POINTER(wintypes.DWORD)]
    handle = kernel.OpenProcess(0x1010,False,pid)
    if not handle: raise ctypes.WinError(ctypes.get_last_error())
    started = time.monotonic()
    try:
        while not stop.is_set():
            memory = Memory(); memory.cb = ctypes.sizeof(memory)
            handles = wintypes.DWORD()
            if not kernel.K32GetProcessMemoryInfo(handle,ctypes.byref(memory),memory.cb): break
            kernel.GetProcessHandleCount(handle,ctypes.byref(handles))
            rows.append({'elapsed_seconds':round(time.monotonic()-started,3),
                'captured_at':dt.datetime.now(dt.timezone.utc).isoformat(),
                'private_bytes':memory.PrivateUsage,'working_set_bytes':memory.WorkingSetSize,'handles':handles.value})
            stop.wait(2)
    finally: kernel.CloseHandle(handle)


def run(exe,pid,output,soak):
    evidence = {'started_at':dt.datetime.now(dt.timezone.utc).isoformat(),'pid':pid,'checks':[],
        'memory_source':'GetProcessMemoryInfo.PrivateUsage, external 2-second samples','memory':[]}
    output.parent.mkdir(parents=True,exist_ok=True)
    def save(): output.write_text(json.dumps(evidence,indent=2),encoding='utf-8')
    def check(name, action):
        started=time.monotonic()
        try:
            detail=action(); row={'name':name,'status':'passed','detail':detail}
        except Exception as error:
            row={'name':name,'status':'failed','error':str(error)}
        row['seconds']=round(time.monotonic()-started,3)
        evidence['checks'].append(row);save();print(json.dumps(row),flush=True)
    stop=threading.Event()
    observer=threading.Thread(target=monitor,args=(pid,stop,evidence['memory']),daemon=True);observer.start()
    started=time.monotonic()
    b=Bridge(exe)
    def discover(bridge,target):
        result=bridge.tool('list_processes',{'filter':{'pid':target}})['data']
        bridge.tool('release_capture',{'capture_id':result['capture_id']})
        assert len(result['rows'])==1,result
        return result['rows'][0]['process_ref']
    def capture(kind,ref=None):
        args={'kind':kind}
        if ref:args['process_ref']=ref
        job=b.tool('start_capture',args)['data']['job_id']
        deadline=time.monotonic()+65
        while time.monotonic()<deadline:
            result=b.tool('get_job',{'job_id':job})['data']
            if result['state']!='running':break
            time.sleep(.1)
        assert result['state']=='succeeded',result
        report=result['result'];source=report['capture_id']
        page=b.tool('read_capture',{'capture_id':source,'limit':5})
        assert page['status']==report['status'], 'partial capture incorrectly marked complete'
        read=page['data']
        assert report.get('coverage',{}).items() <= read['coverage'].items(), 'capture coverage lost on read'
        detail={'rows':read['coverage']['listed'],'capture_status':report.get('status'),'coverage':report.get('coverage')}
        b.tool('release_capture',{'capture_id':read['capture_id']})
        b.tool('release_capture',{'capture_id':source})
        return detail
    try:
        tools=b.call('tools/list',{})['tools']
        evidence['advertised_tools']=[tool['name'] for tool in tools]
        reference=discover(b,pid)
        def baseline():
            status=b.tool('get_status'); assert status['data']['activity']=='live',status
            host=b.tool('get_system_info'); assert host['data']['total_memory_bytes'].isdigit(),host
            metrics=b.tool('get_metrics',{'source':'live','targets':[{'process_ref':reference}],'metric_ids':['private_bytes']})
            assert metrics['data']['samples'][0]['values']['private_bytes'].isdigit(),metrics
            b.tool('get_tracking');b.tool('list_metrics')
            return {'tools':len(tools),'private_bytes':metrics['data']['samples'][0]['values']['private_bytes']}
        check('initialization and all immediate reads',baseline)
        def invalid():
            for args in [{},{'source':'live','targets':[{'system':True}],'metric_ids':['bad']},
                {'source':'live','targets':[{'system':True}],'metric_ids':['private_bytes'],'extra':True}]:
                b.tool('get_metrics',args,error='INVALID_ARGUMENT')
            return 'invalid calls rejected; following ping succeeds' if b.call('ping',{})=={} else 'unexpected ping'
        check('input rejection and recovery',invalid)
        def isolated():
            other=Bridge(exe)
            try:other.tool('get_metrics',{'source':'live','targets':[{'process_ref':reference}],'metric_ids':['private_bytes']},error='REFERENCE_INVALID')
            finally:other.close()
            return 'foreign process reference rejected'
        check('connection isolation',isolated)
        def pages():
            first=b.tool('list_processes',{'limit':1,'sort':'pid'})['data'];assert first['next_cursor']
            args={'limit':1,'sort':'pid','cursor':first['next_cursor']}
            second=b.tool('list_processes',args);time.sleep(1.2)
            repeated=b.tool('list_processes',args);assert second['data']['rows']==repeated['data']['rows']
            assert second['captured_at']==repeated['captured_at'], 'frozen page timestamp changed'
            b.tool('list_processes',{**args,'limit':2},error='CURSOR_INVALID')
            b.tool('release_capture',{'capture_id':first['capture_id']})
            b.tool('list_processes',args,error='CURSOR_INVALID')
            return 'stable rows, changed parameters and released cursor rejected'
        check('immutable pagination',pages)
        def history():
            now=dt.datetime.now(dt.timezone.utc)
            args={'source':'live','target':{'process_ref':reference},'metric_ids':['private_bytes'],
                'from':(now-dt.timedelta(seconds=120)).isoformat(),'to':now.isoformat(),'mode':'raw','limit':500}
            raw=b.tool('get_history',args)['data']; summary=b.tool('get_history',{**args,'mode':'summary'})['data']
            values=[int(row['values']['private_bytes']) for row in raw['rows'] if 'private_bytes' in row['values']]
            stats=summary['summaries']['private_bytes'];assert stats['sample_count']==len(raw['rows'])
            assert stats['available_value_count']==len(values);assert int(stats['min'])==min(values)
            assert int(stats['max'])==max(values)
            b.tool('release_capture',{'capture_id':raw['capture_id']})
            return {'samples':len(raw['rows']),'missing':stats['missing_value_count']}
        check('raw history and exact summary agreement',history)
        for kind in ['threads','scheduling','modules','open_files','network']:
            check(kind+' capture',lambda kind=kind:capture(kind,reference))
        check('global file users capture',lambda:capture('file_users'))
        def limit():
            ids=[]
            try:
                for _ in range(8):ids.append(b.tool('list_processes',{'limit':1})['data']['capture_id'])
                b.tool('list_processes',{'limit':1},error='RESOURCE_LIMIT')
            finally:
                for identity in ids:b.tool('release_capture',{'capture_id':identity})
            discover(b,pid)
            return 'eight captures allowed, ninth rejected, capacity reusable'
        check('capture limit and release recovery',limit)
        def exited_history():
            # A unique image name avoids the ordinary two-generation pruning of busy python.exe lifetimes.
            helper=output.parent / ('mcp-lifetime-'+uuid.uuid4().hex+'.exe')
            shutil.copyfile(exe,helper)
            child=subprocess.Popen([str(helper.resolve()),'mcp-bridge'],stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=subprocess.CREATE_NO_WINDOW)
            beginning=dt.datetime.now(dt.timezone.utc)
            try:
                for _ in range(30):
                    listed=b.tool('list_processes',{'filter':{'pid':child.pid}})['data']
                    b.tool('release_capture',{'capture_id':listed['capture_id']})
                    if listed['rows']:break
                    time.sleep(.2)
                assert listed['rows'],'helper was not sampled'
                time.sleep(1.2)
            finally:
                child.terminate();child.wait(timeout=5);child.stdin.close();helper.unlink()
            time.sleep(2.2)
            other=Bridge(exe)
            try:
                retained=other.tool('list_processes',{'scope':'retained','filter':{'pid':child.pid}})['data']
                assert retained['rows'],'ended identity is not discoverable'
                ref=retained['rows'][0]['process_ref']
                args={'source':'live','target':{'process_ref':ref},'metric_ids':['private_bytes'],'from':beginning.isoformat(),'to':dt.datetime.now(dt.timezone.utc).isoformat(),'mode':'raw','limit':100}
                history=other.tool('get_history',args)['data'];assert history['rows']
                other.tool('get_metrics',{'source':'live','targets':[{'process_ref':ref}],'metric_ids':['private_bytes']},error='PROCESS_EXITED')
                other.tool('start_capture',{'kind':'modules','process_ref':ref},error='PROCESS_EXITED')
                return {'retained_samples':len(history['rows']),'new_connection_discovery':True,'live_inspection_rejected':True}
            finally:other.close()
        check('ended lifetime discovery and history',exited_history)

        def cancelled():
            job=b.tool('start_capture',{'kind':'file_users'})['data']['job_id']
            b.tool('cancel_job',{'job_id':job})
            outcome=b.tool('get_job',{'job_id':job})['data']
            assert outcome['state']!='running',outcome
            return outcome['result'].get('error',{'code':'already_completed'})['code']
        check('cancel collection',cancelled)
        def clients():
            peers=[Bridge(exe) for _ in range(4)]
            try:
                for peer in peers[:3]:peer.tool('get_status')
                peers[3].tool('get_status',error='BACKEND_UNAVAILABLE')
            finally:
                for peer in peers:peer.close()
            return 'four attached clients; fifth rejected'
        check('connection capacity',clients)
        def reconnects():
            for _ in range(20):
                peer=Bridge(exe)
                try:peer.tool('get_status')
                finally:peer.close()
            return '20 reconnect cycles'
        check('reconnect stress',reconnects)
        b.close()
        while time.monotonic()-started < soak:
            save();print(json.dumps({'phase':'idle_soak','elapsed':round(time.monotonic()-started),'latest_memory':evidence['memory'][-1] if evidence['memory'] else None}),flush=True)
            stop.wait(min(30,max(0,soak-(time.monotonic()-started))))
        b=Bridge(exe)
        check('post-soak responsiveness',lambda:b.tool('get_status')['data'])
    finally:
        b.close();stop.set();observer.join(timeout=3)
        evidence['finished_at']=dt.datetime.now(dt.timezone.utc).isoformat();save()
    return 1 if any(row['status']=='failed' for row in evidence['checks']) else 0

if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe',type=Path,required=True);parser.add_argument('--pid',type=int,required=True)
    parser.add_argument('--output',type=Path,required=True);parser.add_argument('--soak-seconds',type=int,default=300)
    args=parser.parse_args()
    sys.exit(run(args.exe,args.pid,args.output,args.soak_seconds))
