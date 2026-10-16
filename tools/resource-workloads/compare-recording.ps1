# Align a single workload evidence file with a schema-v3, 1-second Recording.
# This emits reviewable pairs; it does not assert equality between asynchronous samplers.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Evidence,
    [Parameter(Mandatory)][string]$Recording,
    [Parameter(Mandatory)][string]$Output
)
$ErrorActionPreference = 'Stop'
$schemaPath = Join-Path $PSScriptRoot '../../docs/schemas/recording-v3-line.schema.json'
$schema = Get-Content $schemaPath -Raw | ConvertFrom-Json -AsHashtable
$floatFields = $schema.'$defs'.processF64Values.prefixItems.title
$integerFields = $schema.'$defs'.processU64Values.prefixItems.title
$rows = @(Get-Content -LiteralPath $Evidence | ForEach-Object { $_ | ConvertFrom-Json -AsHashtable })
$start = @($rows | Where-Object kind -eq 'start')
if ($start.Count -ne 1) { throw 'Expected one workload start record.' }
$target = [long]$start[0].pid
$started = ([DateTimeOffset]$start[0].data.process_start_utc).ToUnixTimeSeconds()
$samples = @($rows | Where-Object kind -eq 'sample' | Sort-Object { [DateTimeOffset]$_.utc })
if (-not $samples.Count) { throw 'No workload samples.' }
$times = @($samples | ForEach-Object { ([DateTimeOffset]$_.utc).ToUnixTimeMilliseconds() })
$end = @($rows | Where-Object kind -eq 'end')
if ($end.Count -ne 1) { throw 'Workload evidence must have an end record.' }
$ended = ([DateTimeOffset]$end[0].utc).ToUnixTimeMilliseconds()
$definitions = @{}
$sessionSeen = $false
$matches = 0
$cursor = 0
$stream = [IO.File]::Open([IO.Path]::GetFullPath($Output), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
$writer = [IO.StreamWriter]::new($stream)
try {
    foreach ($line in [IO.File]::ReadLines([IO.Path]::GetFullPath($Recording))) {
        if (-not $line.Trim()) { continue }
        $record = $line | ConvertFrom-Json -AsHashtable
        if ($record.ContainsKey('s')) {
            if ($sessionSeen -or $record.s.v -ne 3 -or $record.s.interval -ne 1) { throw 'Requires one schema-v3 session with a 1-second interval.' }
            $sessionSeen = $true
        }
        if (-not $sessionSeen) { throw 'Recording session metadata missing.' }
        if ($record.ContainsKey('p')) {
            $definition = $record.p
            if ([long]$definition[1] -eq $target -and $null -ne $definition[3] -and [Math]::Abs([long]$definition[3] - $started) -le 1) {
                $definitions[[string]$definition[0]] = $definition
            }
        }
        if (-not $record.ContainsKey('f')) { continue }
        $timestamp = [long]$record.f[0]
        if ($timestamp -lt $times[0] -or $timestamp -gt $ended) { continue }
        while ($cursor + 1 -lt $times.Count -and $times[$cursor + 1] -le $timestamp) { ++$cursor }
        foreach ($process in $record.f[2]) {
            if (-not $definitions.ContainsKey([string]$process[0])) { continue }
            $metrics = [ordered]@{}
            for ($i = 0; $i -lt $floatFields.Count; ++$i) {
                if (-not $floatFields[$i].StartsWith('reserved')) { $metrics[$floatFields[$i]] = if ($i -lt $process[1].Count) { $process[1][$i] } else { $null } }
            }
            for ($i = 0; $i -lt $integerFields.Count; ++$i) {
                $metrics[$integerFields[$i]] = if ($i -lt $process[2].Count) { $process[2][$i] } else { $null }
            }
            $age = $timestamp - $times[$cursor]
            $paired = $age -le 1500
            $entry = [ordered]@{
                captured_at_ms = $timestamp; pid = $target; workload = $start[0].workload
                evidence_age_ms = $age; paired = $paired; recorded = $metrics
                evidence = if ($paired) { $samples[$cursor].data } else { $null }
            }
            $writer.WriteLine(($entry | ConvertTo-Json -Depth 20 -Compress))
            if ($paired) { ++$matches }
        }
    }
} finally { $writer.Dispose() }
if ($matches -eq 0) { throw 'No contemporaneous samples matched PID and process start time; the output contains no usable pairs.' }
Write-Output "Aligned $matches samples. Review phase plateaus; missing metrics remain null. Output: $Output"
