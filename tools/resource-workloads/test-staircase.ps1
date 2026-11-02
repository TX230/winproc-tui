[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$exe = Join-Path $root 'target/resource-workloads/publish/demo-memory.exe'
$out = Join-Path $root ("logs/workloads/staircase-{0}" -f [Guid]::NewGuid().ToString('N'))
foreach ($mode in 'screenshot','verify') {
    $seconds = if ($mode -eq 'screenshot') { 210 } else { 120 }
    $directory = Join-Path $out $mode
    & $exe --mode $mode --scale 0.125 --time-scale 0.025 --seconds $seconds --output $directory
    if ($LASTEXITCODE -ne 0) { throw "Memory workload failed: $mode" }
    $rows = @(Get-Content (Get-ChildItem $directory -Filter '*.jsonl')[0].FullName | ForEach-Object { $_ | ConvertFrom-Json })
    if ($rows[-1].kind -ne 'end' -or -not $rows[-1].data.success) { throw 'Missing successful completion' }
    $phases = @($rows | Where-Object kind -eq 'phase')
    $expected = if ($mode -eq 'screenshot') { @(0,32,64,96,64,32,0,32) } else { @(0,32,32,96,32,0) }
    $payloads = @($phases | ForEach-Object { $_.data.actual.native_committed_bytes / 1MB })
    if (($payloads -join ',') -ne ($expected -join ',')) { throw "Unexpected $mode payload sequence: $payloads" }
    $plateaus = for ($i = 0; $i -lt $phases.Count; $i++) {
        $begin = $phases[$i].elapsed_seconds
        $end = if ($i + 1 -lt $phases.Count) { $phases[$i+1].elapsed_seconds } else { [double]::PositiveInfinity }
        $samples = @($rows | Where-Object { $_.kind -eq 'sample' -and $_.elapsed_seconds -ge $begin -and $_.elapsed_seconds -lt $end })
        if (-not $samples.Count) { throw "No OS sample for $mode step $i" }
        [long]$samples[-1].data.observed.private_bytes
    }
    for ($i = 1; $i -lt $expected.Count; $i++) {
        $requestedDelta = ($expected[$i] - $expected[$i-1]) * 1MB
        $observedDelta = $plateaus[$i] - $plateaus[$i-1]
        # Logging/CRT overhead can drift slightly; compare changes rather than exact process totals.
        if ([Math]::Abs($observedDelta - $requestedDelta) -gt 8MB) {
            throw "Private Bytes did not follow $mode step ${i}: requested=$requestedDelta observed=$observedDelta"
        }
    }
    [pscustomobject]@{mode=$mode;payload_mib=$payloads;private_bytes=$plateaus} |
        ConvertTo-Json -Depth 4 | Set-Content (Join-Path $directory 'verified-plateaus.json')
}
Write-Output "Screenshot Private Bytes staircase and verification memory stages passed: $out"
