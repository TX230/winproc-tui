[CmdletBinding()]
param([switch]$SkipGpu)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$exe = Join-Path $root 'target/resource-workloads/publish/demo-workload.exe'
& $exe --self-test
if ($LASTEXITCODE -ne 0) { throw 'Self-test failed' }
$out = Join-Path $root ("logs/workloads/smoke-{0}" -f [Guid]::NewGuid().ToString('N'))
$workloads = @('cpu','memory','threads','handles','files','io-read','io-write','dotnet-short','dotnet-gen2','dotnet-loh','dotnet-poh','dotnet-frag','user','gdi')
if (-not $SkipGpu) { $workloads += @('gpu','gpu-active') }
& (Join-Path $PSScriptRoot 'run.ps1') -Mode verify -Workload $workloads -Scale 0.125 -TimeScale 0.025 -Output $out
$results = foreach ($item in $workloads) {
    $path = @(Get-ChildItem (Join-Path $out $item) -Filter '*.jsonl')
    if ($path.Count -ne 1) { throw "Missing evidence: $item" }
    $rows = @(Get-Content $path[0].FullName | ForEach-Object { $_ | ConvertFrom-Json })
    if ($rows[-1].kind -ne 'end' -or -not $rows[-1].data.success) { throw "Missing clean end: $item" }
    if (@($rows | Where-Object kind -eq 'error').Count) { throw "Error evidence: $item" }
    $phases = @($rows | Where-Object kind -eq 'phase')
    $high = @($phases | Where-Object { $_.data.Level -eq 2 })
    if ($high.Count -eq 0) { throw "High phase not reached: $item" }
    $actual = $high[0].data.actual
    $samples = @($rows | Where-Object kind -eq 'sample')
    $max = $samples[-1].data.actual
    switch ($item) {
        cpu { if ($max.cpu_iterations -le 0) { throw 'CPU work did not run' } }
        memory { if ($actual.native_committed_bytes -ne 96MB -or -not $actual.memory_touched) { throw 'Memory growth missing' } }
        threads { if ($actual.additional_threads -ne 4) { throw 'Thread count mismatch' } }
        handles { if ($actual.event_handles -ne 125) { throw 'Handle count mismatch' } }
        files { if ($actual.held_files.Count -ne 2) { throw 'Held file count mismatch' } }
        { $_ -in 'io-read','io-write' } { if ($max.transferred_bytes_total -le 0) { throw 'No I/O' } }
        dotnet-short { if ($max.short_allocated_payload_bytes_total -le 0) { throw 'No managed allocation' } }
        dotnet-gen2 { if ($actual.gen2_candidate_payload_bytes -ne 24MB) { throw 'Gen2 retention mismatch' } }
        dotnet-loh { if ($actual.loh_payload_bytes -ne 32MB) { throw 'LOH retention mismatch' } }
        dotnet-poh { if ($actual.poh_payload_bytes -ne 16MB) { throw 'POH retention mismatch' } }
        dotnet-frag { if ($actual.loh_payload_bytes -ne 16MB) { throw 'Fragmentation release mismatch' } }
        user { if ($actual.user_windows -ne 6) { throw 'USER count mismatch' } }
        gdi { if ($actual.gdi_brushes -ne 62) { throw 'GDI count mismatch' } }
        { $_ -in 'gpu','gpu-active' } {
            $expected = if ($item -eq 'gpu') { 64MB } else { 16MB }
            if ($actual.gpu.resource_payload_bytes -ne $expected -or $max.gpu.completed_passes -le 0) { throw 'GPU work missing' }
        }
    }
    $baseline = $samples[0].data.observed
    $highSample = @($samples | Where-Object { $_.data.phase -in 'high','grow' })[0].data.observed
    switch ($item) {
        memory { if ($highSample.private_bytes - $baseline.private_bytes -lt 80MB) { throw 'Committed memory not observed by OS' } }
        threads { if ($highSample.threads - $baseline.threads -lt 4) { throw 'Additional threads not observed by OS' } }
        handles { if ($highSample.handles - $baseline.handles -lt 120) { throw 'Additional handles not observed by OS' } }
        user { if ($highSample.user_objects - $baseline.user_objects -lt 6) { throw 'USER objects not observed by OS' } }
        gdi { if ($highSample.gdi_objects - $baseline.gdi_objects -lt 62) { throw 'GDI objects not observed by OS' } }
        dotnet-gen2 { if ($highSample.generations_after_gc[2].bytes -lt 24MB) { throw 'Gen2 retention not observed by GC' } }
        dotnet-loh { if ($highSample.generations_after_gc[3].bytes -lt 32MB) { throw 'LOH retention not observed by GC' } }
        dotnet-poh { if ($highSample.generations_after_gc[4].bytes -lt 16MB) { throw 'POH retention not observed by GC' } }
    }
    if (@(Get-ChildItem (Join-Path $out $item) -Directory -Filter 'scratch-*').Count) { throw "Scratch not cleaned: $item" }
    [pscustomobject]@{ workload = $item; result = 'passed'; samples = $samples.Count }
}
# An invalid adapter must fail visibly and still produce a clean terminal record.
if (-not $SkipGpu) {
    & $exe --workload gpu --adapter 15 --seconds 1 --output (Join-Path $out 'gpu-failure') 2>$null
    if ($LASTEXITCODE -ne 1) { throw 'Invalid adapter did not report failure' }
    $failure = Get-Content (Get-ChildItem (Join-Path $out 'gpu-failure') -Filter '*.jsonl')[0].FullName | ForEach-Object { $_ | ConvertFrom-Json }
    if ($failure[-1].data.success -ne $false) { throw 'Failure end record missing' }
}
# Stop-file cancellation during sustained screenshot load must cleanly release resources.
$photo = Join-Path $out 'photo-stop'
& (Join-Path $PSScriptRoot 'run.ps1') -Mode screenshot -Workload memory,dotnet -Scale 0.125 -TimeScale 0.025 -Seconds 3600 -Background -Output $photo
Start-Sleep -Seconds 4
[IO.File]::WriteAllText((Join-Path $photo 'stop.request'), 'stop')
$deadline = [DateTime]::UtcNow.AddSeconds(10)
do {
    $ends = @(Get-ChildItem $photo -Recurse -Filter '*.jsonl' | ForEach-Object {
        Get-Content $_.FullName -Tail 1 | ConvertFrom-Json
    } | Where-Object { $_.kind -eq 'end' -and $_.data.reason -eq 'stop-request' -and $_.data.success })
    if ($ends.Count -eq 2) { break }
    Start-Sleep -Milliseconds 200
} while ([DateTime]::UtcNow -lt $deadline)
if ($ends.Count -ne 2) { throw 'Cooperative screenshot stop failed' }
$results | ConvertTo-Json | Set-Content (Join-Path $out 'results.json')
$results | Format-Table
Write-Output "Evidence: $out"
