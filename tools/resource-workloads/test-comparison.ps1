[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$out = Join-Path $root ("logs/workloads/comparison-test-{0}" -f [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $out | Out-Null
$evidence = @"
{"kind":"start","utc":"1970-01-01T00:33:20Z","pid":321,"workload":"memory","data":{"process_start_utc":"1970-01-01T00:33:20Z"}}
{"kind":"sample","utc":"1970-01-01T00:33:21Z","pid":321,"data":{"phase":"high","actual":{"native_committed_bytes":123},"observed":{"private_bytes":456}}}
{"kind":"end","utc":"1970-01-01T00:33:25Z","pid":321,"data":{"success":true}}
"@
$recording = @"
{"s":{"v":3,"interval":1}}
{"p":[1,321,"demo-memory.exe",2000,null]}
{"p":[2,321,"demo-memory.exe",1000,null]}
{"f":[2001100,[],[[1,[12.5],[123,null]],[2,[99],[999]]]]}
{"f":[2003900,[],[[1,[0],[321]]]]}
"@
[IO.File]::WriteAllText((Join-Path $out 'evidence.jsonl'), $evidence)
[IO.File]::WriteAllText((Join-Path $out 'recording.jsonl'), $recording)
& (Join-Path $PSScriptRoot 'compare-recording.ps1') -Evidence (Join-Path $out 'evidence.jsonl') -Recording (Join-Path $out 'recording.jsonl') -Output (Join-Path $out 'pairs.jsonl')
$rows = @(Get-Content (Join-Path $out 'pairs.jsonl') | ForEach-Object { $_ | ConvertFrom-Json })
if ($rows.Count -ne 2 -or -not $rows[0].paired -or $rows[1].paired -or $rows[0].recorded.private_bytes -ne 123 -or
    $rows[0].recorded.cpu_percent -ne 12.5 -or $null -ne $rows[0].recorded.workset_bytes -or $null -ne $rows[0].recorded.dotnet_gc_poh_bytes) {
    throw 'Identity, age, schema mapping or null preservation failed'
}
$bad = $recording.Replace('"interval":1', '"interval":2')
[IO.File]::WriteAllText((Join-Path $out 'aggregated.jsonl'), $bad)
$rejected = $false
try {
    & (Join-Path $PSScriptRoot 'compare-recording.ps1') -Evidence (Join-Path $out 'evidence.jsonl') -Recording (Join-Path $out 'aggregated.jsonl') -Output (Join-Path $out 'rejected.jsonl')
} catch { $rejected = $true }
if (-not $rejected) { throw 'Aggregated recording accepted unexpectedly' }
Write-Output "Comparison tests passed: $out"
