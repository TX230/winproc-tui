param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidateSet('high','low')][string]$ReasoningEffort = 'high',
    [switch]$Direct,
    [string]$CodexCommand = 'codex',
    [string]$NodeCommand = 'node'
)
$ErrorActionPreference = 'Stop'
$exe = (Resolve-Path -LiteralPath $Executable).Path.Replace('\','/')
$proxy = (Join-Path $PSScriptRoot 'trace-mcp-stdio.mjs').Replace('\','/')
$node = (Get-Command $NodeCommand -ErrorAction Stop).Source.Replace('\','/')
$outDir = [IO.Path]::GetFullPath($OutputDirectory)
[IO.Directory]::CreateDirectory($outDir) | Out-Null
$wire = (Join-Path $outDir 'wire.jsonl').Replace('\','/')
$utf8 = [Text.UTF8Encoding]::new($false)
$events = [IO.StreamWriter]::new((Join-Path $outDir 'events.jsonl'),$false,$utf8)
$events.NewLine = "`n"
$prompt = 'Use the winproc MCP to get application status once. Return only the activity and stale flag. Do not use shell, files, web, skills, subagents, or change state.'
if($Direct){$prompt = 'Call winproc_get_status with {} as your first action. Do not emit preliminary commentary or a plan. Return only activity and stale. No shell, files, web, skills, subagents or state changes.'}
$config = @('-c',"mcp_servers.winproc.command=`"$node`"",'-c',('mcp_servers.winproc.args='+(ConvertTo-Json -InputObject @($proxy,$exe,$wire) -Compress)),'-c','mcp_servers.node_repl.enabled=false','-c',"model_reasoning_effort=`"$ReasoningEffort`"")
$started = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
try {
    & $CodexCommand exec --ephemeral --json --color never -s read-only @config $prompt 2> (Join-Path $outDir 'stderr.log') | ForEach-Object {
        $events.WriteLine($_)
        $events.Flush()
    }
    if($LASTEXITCODE -ne 0){throw "Codex exited $LASTEXITCODE"}
} finally { $events.Dispose() }
$messages = @(Get-Content -LiteralPath $wire | ForEach-Object {$_ | ConvertFrom-Json})
$listed = @($messages | Where-Object {$_.event -eq 'response_flushed' -and $_.method -eq 'tools/list'})[0]
$called = @($messages | Where-Object {$_.event -eq 'client_request' -and $_.method -eq 'tools/call'})[0]
if(-not $listed -or -not $called){throw 'Missing discovery or first-call boundary'}
if($called.tool -ne 'winproc_get_status'){throw "Unexpected first tool: $($called.tool)"}
$nativeEvents=@(Get-Content -LiteralPath (Join-Path $outDir 'events.jsonl') | ForEach-Object {$_ | ConvertFrom-Json})
$resultEvent=@($nativeEvents | Where-Object {$_.type -eq 'item.completed' -and $_.item.type -eq 'mcp_tool_call'})[0]
if($resultEvent.item.result.structured_content.error -or -not $resultEvent.item.result.structured_content.data.activity){throw 'Status request did not succeed'}
$result = [ordered]@{reasoning_effort=$ReasoningEffort;direct=$Direct.IsPresent;list_to_first_call_ms=([decimal]$called.ns-[decimal]$listed.ns)/1000000;launch_to_first_call_ms=$called.epoch_ms-$started;first_tool=$called.tool}
[IO.File]::WriteAllText((Join-Path $outDir 'summary.json'),($result|ConvertTo-Json).Replace("`r`n","`n")+"`n",$utf8)
$result|ConvertTo-Json
