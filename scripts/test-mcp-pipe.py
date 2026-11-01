"""Test the current logon session's live MCP pipe without TCP/IP.
Argument: local JSON evidence path. Does not change application state.
"""
import json
from pathlib import Path
import queue
import re
import struct
import subprocess
import sys
import threading
import time

sid=re.search(r'S-1-5-5-\d+-\d+',subprocess.check_output(['whoami','/logonid'],text=True)).group()
name=r'\\.\pipe\winproc-tui.mcp.v1.'+sid
results=[]
def frame(value):
    body=json.dumps(value,separators=(',',':')).encode()
    return struct.pack('<I',len(body))+body
hello={'kind':'hello','ipc_major':1,'ipc_minor':0,'bridge_version':'system-test'}
def read(pipe):
    header=pipe.read(4)
    if not header:raise EOFError()
    assert len(header)==4
    length=struct.unpack('<I',header)[0];assert 0<length<=4*1024*1024
    body=bytearray()
    while len(body)<length:
        chunk=pipe.read(length-len(body))
        if not chunk:raise EOFError()
        body.extend(chunk)
    return json.loads(body)
def bounded_read(pipe):
    q=queue.Queue()
    def work():
        try:q.put((True,read(pipe)))
        except Exception as error:q.put((False,type(error).__name__))
    threading.Thread(target=work,daemon=True).start()
    return q.get(timeout=5)
def check(name,action):
    try:detail=action();result={'name':name,'status':'passed','detail':detail}
    except Exception as error:result={'name':name,'status':'failed','error':str(error)}
    results.append(result);print(json.dumps(result),flush=True)
def rejected(payload):
    with open(name,'r+b',buffering=0) as pipe:
        pipe.write(payload)
        ok,value=bounded_read(pipe)
        assert not ok,value
        return value
for title,payload in [('zero length',struct.pack('<I',0)),('oversized length',struct.pack('<I',4*1024*1024+1)),
    ('invalid UTF-8',struct.pack('<I',1)+b'\xff'),('malformed JSON',struct.pack('<I',1)+b'{'),
    ('major version mismatch',frame({**hello,'ipc_major':999}))]:
    check(title,lambda payload=payload:rejected(payload))
def fragmented():
    with open(name,'r+b',buffering=0) as pipe:
        for byte in frame(hello):pipe.write(bytes([byte]))
        ok,welcome=bounded_read(pipe);assert ok and welcome['kind']=='welcome',welcome
        a={'kind':'request','request_id':'first','method':'winproc_get_status','arguments':{}}
        b={**a,'request_id':'second'}
        pipe.write(frame(a)+frame(b))
        replies=[bounded_read(pipe),bounded_read(pipe)]
        assert all(ok for ok,_ in replies),replies
        assert {reply['request_id'] for _,reply in replies}=={'first','second'},replies
        return 'fragmented handshake and coalesced requests completed'
check('fragmentation and coalescing',fragmented)
def cancellation():
    with open(name,'r+b',buffering=0) as pipe:
        pipe.write(frame(hello));assert bounded_read(pipe)[0]
        request={'kind':'request','request_id':'cancel-me','method':'winproc_get_status','arguments':{}}
        pipe.write(frame(request)+frame({'kind':'cancel','request_id':'cancel-me'})+frame({**request,'request_id':'followup'}))
        for _ in range(2):
            ok,value=bounded_read(pipe);assert ok,value
            if value['request_id']=='followup':return 'connection remains usable after cancellation race'
        raise AssertionError('followup missing')
check('cancellation race',cancellation)
def stalled_reader():
    # Isolate synchronous WriteFile in a child: failed assertions must not hang
    # cleanup while another thread owns Python's file-object lock.
    code = r"""
import json,struct,sys
p=open(sys.argv[1],'r+b',buffering=0)
def send(v):
 b=json.dumps(v).encode();p.write(struct.pack('<I',len(b))+b)
send({'kind':'hello','ipc_major':1,'ipc_minor':0,'bridge_version':'stress'})
n=struct.unpack('<I',p.read(4))[0];p.read(n)
try:
 for i in range(100000):send({'kind':'request','request_id':str(i),'method':'winproc_list_metrics','arguments':{}})
except OSError:
 print('disconnected',flush=True);sys.exit(0)
sys.exit(2)
"""
    child=subprocess.Popen([sys.executable,'-c',code,name],stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,creationflags=subprocess.CREATE_NO_WINDOW)
    try:
        time.sleep(.2)
        with open(name,'r+b',buffering=0) as peer:
            peer.write(frame(hello));assert bounded_read(peer)[0]
            peer.write(frame({'kind':'request','request_id':'independent','method':'winproc_get_status','arguments':{}}))
            ok,result=bounded_read(peer);assert ok and result['result']['data']['connected'],result
        out,err=child.communicate(timeout=15)
        assert child.returncode==0 and 'disconnected' in out,(out,err,child.returncode)
        return {'independent_client_responsive':True,'stalled_writer_closed':True}
    finally:
        if child.poll() is None:child.kill();child.communicate(timeout=5)
check('stalled reader deadline and independent client',stalled_reader)
Path(sys.argv[1]).write_text(json.dumps(results,indent=2),encoding='utf-8')
sys.exit(1 if any(row['status']=='failed' for row in results) else 0)
