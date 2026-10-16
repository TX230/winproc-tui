[CmdletBinding()]
param(
    [ValidateSet('verify','screenshot')][string]$Mode = 'verify',
    [string[]]$Workload,
    [ValidateRange(0.0625,1)][double]$Scale,
    [ValidateRange(0.025,1)][double]$TimeScale = 1,
    [ValidateRange(1,3600)][int]$Seconds,
    [ValidateRange(0,15)][int]$Adapter = 0,
    [switch]$NaturalGc,
    [switch]$Background,
    [string]$Output
)
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$bin = Join-Path $root 'target/resource-workloads/publish'
$presets = Get-Content (Join-Path $PSScriptRoot 'presets.json') -Raw | ConvertFrom-Json
$preset = $presets.$Mode
if (-not $Workload) { $Workload = $preset.workloads }
if (-not $PSBoundParameters.ContainsKey('Scale')) { $Scale = $preset.scale }
if (-not $PSBoundParameters.ContainsKey('Seconds')) { $Seconds = $preset.seconds }
$valid = @($presets.verify.workloads) + @($presets.screenshot.workloads) + @('threads','handles','files')
foreach ($item in $Workload) { if ($item -notin $valid) { throw "Unknown workload: $item" } }
if (@($Workload | Select-Object -Unique).Count -ne $Workload.Count) { throw 'Duplicate workloads are not allowed.' }
if ($Background -and $Mode -eq 'verify' -and $Workload.Count -gt 1) { throw 'Background verification accepts one workload; use screenshot for concurrent loads.' }
if (-not (Test-Path (Join-Path $bin 'demo-workload.exe'))) { throw 'Run build.ps1 first.' }
if (-not $Output) { $Output = Join-Path $root ("logs/workloads/{0}-{1}" -f (Get-Date -Format 'yyyyMMdd-HHmmss'), [Guid]::NewGuid().ToString('N').Substring(0,8)) }
$Output = [IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $Output) { throw 'Output must be a new directory to prevent mixing runs.' }
New-Item -ItemType Directory -Path $Output | Out-Null
$stop = Join-Path $Output 'stop.request'
$children = [Collections.Generic.List[Diagnostics.Process]]::new()
$manifest = [Collections.Generic.List[object]]::new()
$failed = $false
$launched = $false
try {
    foreach ($item in $Workload) {
        $alias = if ($item.StartsWith('dotnet')) { 'dotnet' } elseif ($item.StartsWith('gpu')) { 'gpu' } elseif ($item -in 'user','gdi') { 'objects' } else { $item }
        $exe = Join-Path $bin "demo-$alias.exe"
        $log = Join-Path $Output $item
        $argsList = @('--workload', $item, '--mode', $Mode, '--scale', $Scale.ToString([Globalization.CultureInfo]::InvariantCulture),
            '--time-scale', $TimeScale.ToString([Globalization.CultureInfo]::InvariantCulture), '--seconds', "$Seconds",
            '--adapter', "$Adapter", '--force-gc', (!$NaturalGc -and $preset.forceGc).ToString().ToLowerInvariant(),
            '--output', "`"$log`"", '--stop-file', "`"$stop`"")
        $child = Start-Process -FilePath $exe -ArgumentList $argsList -WindowStyle Hidden -PassThru -WorkingDirectory $bin `
            -RedirectStandardOutput (Join-Path $Output "$item.stdout.txt") -RedirectStandardError (Join-Path $Output "$item.stderr.txt")
        $children.Add($child)
        $manifest.Add([pscustomobject]@{ workload = $item; pid = $child.Id; name = [IO.Path]::GetFileName($exe); evidence = $log })
        $manifest | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Output 'processes.json')
        Write-Host "$item : PID $($child.Id) ($([IO.Path]::GetFileName($exe)))"
        if ($Mode -eq 'verify' -and -not $Background) {
            while (-not $child.WaitForExit(250)) { }
            if ($child.ExitCode -ne 0) { $failed = $true; break }
        }
    }
    Write-Host "Evidence: $Output"
    Write-Host "Cooperative stop: New-Item -ItemType File -Path '$stop'"
    $launched = $true
    if (-not $Background -and $Mode -eq 'screenshot') {
        while (@($children | Where-Object { -not $_.HasExited }).Count -gt 0) {
            if (@($children | Where-Object { $_.HasExited -and $_.ExitCode -ne 0 }).Count -gt 0) { $failed = $true; break }
            Start-Sleep -Milliseconds 250
        }
        if (@($children | Where-Object { $_.HasExited -and $_.ExitCode -ne 0 }).Count -gt 0) { $failed = $true }
    }
}
finally {
    if (-not $Background -or -not $launched) {
        [IO.File]::WriteAllText($stop, 'stop')
        foreach ($child in $children) {
            if (-not $child.WaitForExit(10000)) { Write-Warning "PID $($child.Id) is still cleaning up; no forced termination was attempted." }
        }
    }
    foreach ($child in $children) { $child.Dispose() }
}
if ($failed) { throw "A workload failed. Inspect *.stderr.txt and JSONL evidence in $Output" }
