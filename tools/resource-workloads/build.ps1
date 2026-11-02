[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$out = Join-Path $root 'target/resource-workloads/publish'
$obj = Join-Path $root 'target/resource-workloads/obj'
New-Item -ItemType Directory -Force $out, $obj | Out-Null
& dotnet publish (Join-Path $PSScriptRoot 'ResourceWorkloads.csproj') -c Release -o $out "-p:BaseIntermediateOutputPath=$obj/"
if ($LASTEXITCODE -ne 0) { throw 'dotnet publish failed' }
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Install Visual Studio C++ Build Tools and the Windows SDK.' }
$dev = Join-Path $vs 'Common7/Tools/VsDevCmd.bat'
$source = Join-Path $PSScriptRoot 'native.cpp'
$gpu = Join-Path $PSScriptRoot 'gpu.cpp'
# Only compiler invocation uses cmd; filesystem operations stay in PowerShell.
$command = "`"`"$dev`" -arch=x64 -host_arch=x64 >nul && cl.exe /nologo /std:c++17 /EHsc /W4 /WX /O2 /MT `"$source`" `"$gpu`" /Fo`"$obj/`" /Fe`"$out/demo-main.exe`" /link d3d11.lib dxgi.lib user32.lib gdi32.lib psapi.lib`""
$start = [Diagnostics.ProcessStartInfo]::new($env:ComSpec)
$start.Arguments = "/d /s /c $command"
$start.UseShellExecute = $false
$compiler = [Diagnostics.Process]::Start($start)
$compiler.WaitForExit()
if ($compiler.ExitCode -ne 0) { throw 'Native demo compilation failed' }
$compiler.Dispose()
# Inspect direct PE imports: OS/driver dependencies may themselves use another CRT.
$check = [Diagnostics.ProcessStartInfo]::new($env:ComSpec)
$check.Arguments = "/d /s /c `"`"$dev`" -arch=x64 -host_arch=x64 >nul && dumpbin.exe /nologo /dependents `"$out/demo-main.exe`"`""
$check.UseShellExecute = $false
$check.RedirectStandardOutput = $true
$inspector = [Diagnostics.Process]::Start($check)
$imports = $inspector.StandardOutput.ReadToEnd()
$inspector.WaitForExit()
if ($inspector.ExitCode -ne 0 -or $imports -match '(?i)VCRUNTIME[0-9]|MSVCP[0-9]|api-ms-win-crt-|mscoree|coreclr|hostfxr|hostpolicy') {
    throw "Unexpected native demo dependencies: $imports"
}
$inspector.Dispose()
foreach ($name in 'worker','cpu','memory','threads','handles','files','io-read','io-write','gpu','objects') {
    Copy-Item -LiteralPath (Join-Path $out 'demo-main.exe') -Destination (Join-Path $out "demo-$name.exe")
}
# Remove known obsolete outputs from the former shared managed host; preserve other files.
foreach ($name in 'demo-workload.exe','demo-workload.dll','demo-workload.pdb','demo-workload.deps.json','demo-workload.runtimeconfig.json','workload-gpu.dll') {
    $obsolete = Join-Path $out $name
    if (Test-Path -LiteralPath $obsolete) { Remove-Item -LiteralPath $obsolete }
}
Write-Output "Built: $out"
