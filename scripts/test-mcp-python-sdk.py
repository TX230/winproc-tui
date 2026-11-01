"""Run with an environment containing the official mcp Python SDK.
Arguments: executable, TUI PID, result JSON path.
"""
import asyncio
import importlib.metadata
import json
from pathlib import Path
import sys
from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client

async def main():
    exe,pid,output=sys.argv[1:]
    async with stdio_client(StdioServerParameters(command=str(Path(exe).resolve()),args=["mcp-bridge"])) as (read,write):
        async with ClientSession(read,write,read_timeout_seconds=15) as client:
            initialized=await client.initialize()
            tools=await client.list_tools()
            async def call(name,args=None):
                response=(await client.call_tool("winproc_"+name,args or {})).model_dump(by_alias=True)
                assert not response.get("isError"),response
                assert json.loads(response["content"][0]["text"])==response["structuredContent"]
                return response["structuredContent"]
            processes=await call("list_processes",{"filter":{"pid":int(pid)}})
            reference=processes["data"]["rows"][0]["process_ref"]
            metrics=await call("get_metrics",{"source":"live","targets":[{"process_ref":reference}],"metric_ids":["private_bytes"]})
            value=metrics["data"]["samples"][0]["values"]["private_bytes"]
            assert isinstance(value,str) and value.isdigit()
            job=(await call("start_capture",{"kind":"open_files","process_ref":reference}))["data"]["job_id"]
            for _ in range(600):
                status=(await call("get_job",{"job_id":job}))["data"]
                if status["state"]!="running":break
                await asyncio.sleep(.1)
            assert status["state"]=="succeeded",status
            report=status["result"]
            page=await call("read_capture",{"capture_id":report["capture_id"],"limit":5})
            assert page["status"]==report["status"]
            for capture in {report["capture_id"],page["data"]["capture_id"],processes["data"]["capture_id"]}:
                await call("release_capture",{"capture_id":capture})
            result={"client":"Python MCP SDK","version":importlib.metadata.version("mcp"),
                "protocol":initialized.model_dump(by_alias=True)["protocolVersion"],"tools":len(tools.tools),
                "process_metrics_files":"passed","file_rows":page["data"]["coverage"]["listed"],"partial":page["status"]=="partial"}
            Path(output).write_text(json.dumps(result,indent=2),encoding="utf-8")
            print(json.dumps(result))

asyncio.run(main())
