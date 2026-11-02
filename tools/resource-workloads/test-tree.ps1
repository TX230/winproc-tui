[CmdletBinding()]
param([switch]$SkipGpu)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$bin = Join-Path $root 'target/resource-workloads/publish'
$out = Join-Path $root ("logs/workloads/tree-{0}" -f [Guid]::NewGuid().ToString('N'))
# Spaces and non-ASCII exercise CreateProcess argument quoting and UTF-8 evidence.
$out = Join-Path $out 'tree evidence 日本語'
$loads = @('cpu','memory','threads','handles','files','io-read','io-write','dotnet','objects')
if (-not $SkipGpu) { $loads += 'gpu' }
& (Join-Path $PSScriptRoot 'run.ps1') -Mode screenshot -Workload $loads -Scale 0.125 -TimeScale 0.025 -Seconds 3600 -Background -Output $out
$rootPid = [int](Get-Content (Join-Path $out 'main.ready'))
$identities = @()
try {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $count = @(Get-ChildItem $out -Recurse -Filter '*.jsonl' | Where-Object { $_.DirectoryName -ne $out }).Count
        if ($count -eq $loads.Count) { break }
        if ([DateTime]::UtcNow -gt $deadline) { throw 'Not all descendants started' }
        Start-Sleep -Milliseconds 100
    } while ($true)
    Start-Sleep -Seconds 4
    $direct = @(Get-Content (Join-Path $out 'processes.json') -Raw | ConvertFrom-Json)
    $nested = @(Get-Content (Join-Path $out 'worker-processes.json') -Raw | ConvertFrom-Json)
    $worker = @($direct | Where-Object workload -eq 'worker')
    if ($worker.Count -ne 1 -or $nested.Count -lt 2) { throw 'Grandchild branch is missing' }
    $manifest = @([pscustomobject]@{pid=$rootPid;name='demo-main.exe';parent_pid=$null}) + $direct + $nested
    $os = @(Get-CimInstance Win32_Process | Where-Object ProcessId -in $manifest.pid)
    foreach ($entry in $manifest) {
        $actual = @($os | Where-Object ProcessId -eq $entry.pid)
        if ($actual.Count -ne 1 -or $actual[0].Name -ne $entry.name) { throw "Missing process: $($entry.name)" }
        if ($null -ne $entry.parent_pid -and $actual[0].ParentProcessId -ne $entry.parent_pid) { throw "Wrong parent: $($entry.name)" }
        $process = Get-Process -Id $entry.pid
        $identities += [pscustomobject]@{pid=$process.Id;start=$process.StartTime}
        $modules = @($process.Modules | ForEach-Object ModuleName)
        $runtime = @($modules | Where-Object { $_ -in 'coreclr.dll','clr.dll','mscoree.dll','hostfxr.dll','hostpolicy.dll' })
        if ($entry.name -eq 'demo-dotnet.exe') {
            if ('coreclr.dll' -notin $runtime) { throw '.NET demo did not load CoreCLR' }
        } elseif ($runtime.Count) { throw "Native demo loaded .NET: $($entry.name): $runtime" }
        $process.Dispose()
    }
    if (@($nested | Where-Object parent_pid -ne $worker[0].pid).Count) { throw 'Manifest grandparent relationship is wrong' }
    $manifest | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $out 'verified-tree.json')
} finally { [IO.File]::WriteAllText((Join-Path $out 'stop.request'), 'stop') }
$deadline = [DateTime]::UtcNow.AddSeconds(15)
do {
    $alive = @($identities | Where-Object {
        $p = Get-Process -Id $_.pid -ErrorAction SilentlyContinue
        $p -and $p.StartTime -eq $_.start
    })
    if (-not $alive.Count) { break }
    Start-Sleep -Milliseconds 100
} while ([DateTime]::UtcNow -lt $deadline)
if ($alive.Count) { throw 'Descendants survived cooperative stop' }
$ends = @(Get-ChildItem $out -Recurse -Filter '*.jsonl' | ForEach-Object { Get-Content $_.FullName -Tail 1 | ConvertFrom-Json })
if ($ends.Count -ne $loads.Count + 2 -or @($ends | Where-Object { $_.kind -ne 'end' -or -not $_.data.success -or $_.data.reason -ne 'stop-request' }).Count) { throw 'Missing successful stop evidence' }
# Reusing a completed run must not overwrite its process manifests.
$before = (Get-FileHash (Join-Path $out 'processes.json')).Hash
& (Join-Path $bin 'demo-main.exe') --workloads cpu --seconds 1 --output $out 2>$null
if ($LASTEXITCODE -ne 1 -or (Get-FileHash (Join-Path $out 'processes.json')).Hash -ne $before) {
    throw 'Reused output was not rejected safely'
}
# Background verification must return after launch, not after its full scenario.
$background = Join-Path (Split-Path $out) 'background-verify'
$timer = [Diagnostics.Stopwatch]::StartNew()
& (Join-Path $PSScriptRoot 'run.ps1') -Mode verify -Workload cpu -Seconds 30 -Background -Output $background
if ($timer.Elapsed.TotalSeconds -gt 10) { throw 'Background verification blocked on completion' }
[IO.File]::WriteAllText((Join-Path $background 'stop.request'), 'stop')
$backgroundPid = [int](Get-Content (Join-Path $background 'main.ready'))
$p = Get-Process -Id $backgroundPid -ErrorAction SilentlyContinue
if ($p -and -not $p.WaitForExit(10000)) { throw 'Background launcher did not stop' }
# Direct executables retain input validation and runtime separation.
foreach ($argsList in @(
    @('--scale','NaN'), @('--seconds','0'), @('--scale','1','--scale','1'),
    @('--workload','dotnet'), @('--workload','cpu','--scale'), @('--cpu-workers','99')
)) {
    & (Join-Path $bin 'demo-cpu.exe') @argsList 2>$null
    if ($LASTEXITCODE -ne 2) { throw "Native validation accepted: $argsList" }
}
& (Join-Path $bin 'demo-dotnet.exe') --workload cpu 2>$null
if ($LASTEXITCODE -ne 2) { throw 'Managed host accepted a native workload' }
# A failing child must fail its ancestors and stop its concurrently running siblings.
if (-not $SkipGpu) {
    $failure = Join-Path (Split-Path $out) 'failure'
    & (Join-Path $bin 'demo-main.exe') --workloads cpu,gpu,dotnet --mode screenshot --adapter 15 --seconds 30 --output $failure
    if ($LASTEXITCODE -ne 1) { throw 'Child failure was not propagated' }
    $terminal = @(Get-ChildItem $failure -Recurse -Filter '*.jsonl' | ForEach-Object { Get-Content $_.FullName -Tail 1 | ConvertFrom-Json })
    if (@($terminal | Where-Object kind -ne 'end').Count) { throw 'Child failure left unfinished evidence' }
}
Write-Output "Tree, native runtime isolation, cooperative cleanup and launcher checks passed: $out"
