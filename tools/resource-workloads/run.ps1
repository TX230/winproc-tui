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
if (-not (Test-Path (Join-Path $bin 'demo-main.exe'))) { throw 'Run build.ps1 first.' }
if (-not $Output) { $Output = Join-Path $root ("logs/workloads/{0}-{1}" -f (Get-Date -Format 'yyyyMMdd-HHmmss'), [Guid]::NewGuid().ToString('N').Substring(0,8)) }
$Output = [IO.Path]::GetFullPath($Output)
if (Test-Path -LiteralPath $Output) { throw 'Output must be a new directory to prevent mixing runs.' }
New-Item -ItemType Directory -Path $Output | Out-Null
$stop = Join-Path $Output 'stop.request'
# ProcessStartInfo.ArgumentList preserves Unicode, spaces, quotes and trailing backslashes.
$start = [Diagnostics.ProcessStartInfo]::new((Join-Path $bin 'demo-main.exe'))
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.WorkingDirectory = $bin
$argsList = @('--workloads', ($Workload -join ','), '--mode', $Mode,
    '--scale', $Scale.ToString([Globalization.CultureInfo]::InvariantCulture),
    '--time-scale', $TimeScale.ToString([Globalization.CultureInfo]::InvariantCulture), '--seconds', "$Seconds",
    '--adapter', "$Adapter", '--force-gc', (!$NaturalGc -and $preset.forceGc).ToString().ToLowerInvariant(),
    '--output', $Output, '--stop-file', $stop)
foreach ($argument in $argsList) { $start.ArgumentList.Add($argument) }
$launcher = $null
$launched = $false
try {
    $launcher = [Diagnostics.Process]::Start($start)
    Write-Host "demo-main.exe : PID $($launcher.Id)"
    Write-Host "Evidence: $Output"
    Write-Host "Cooperative stop: New-Item -ItemType File -Path '$stop'"
    if ($Background) {
        $deadline = [DateTime]::UtcNow.AddSeconds(15)
        while (-not (Test-Path (Join-Path $Output 'main.ready')) -and -not $launcher.HasExited) {
            if ([DateTime]::UtcNow -gt $deadline) { throw 'Demo launcher did not become ready.' }
            Start-Sleep -Milliseconds 50
        }
        if ($launcher.HasExited -and $launcher.ExitCode -ne 0) { throw 'Demo launcher failed; inspect JSONL evidence.' }
    } else {
        while (-not $launcher.WaitForExit(250)) { }
        if ($launcher.ExitCode -ne 0) { throw 'A demo failed; inspect JSONL evidence.' }
    }
    $launched = $true
}
finally {
    if ($launcher) {
        if (-not $Background -or -not $launched) {
            [IO.File]::WriteAllText($stop, 'stop')
            if (-not $launcher.WaitForExit(10000)) { Write-Warning "PID $($launcher.Id) is still cleaning up; no forced termination was attempted." }
        }
        $launcher.Dispose()
    }
}
