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
$source = Join-Path $PSScriptRoot 'gpu.cpp'
# Only compiler invocation uses cmd; there are no filesystem mutations through cmd builtins.
$command = "`"`"$dev`" -arch=x64 -host_arch=x64 >nul && cl.exe /nologo /std:c++17 /EHsc /W4 /WX /O2 /MT /LD `"$source`" /Fo`"$obj/gpu.obj`" /link /OUT:`"$out/workload-gpu.dll`" /IMPLIB:`"$obj/workload-gpu.lib`" d3d11.lib dxgi.lib`""
$start = [Diagnostics.ProcessStartInfo]::new($env:ComSpec)
$start.Arguments = "/d /s /c $command"
$start.UseShellExecute = $false
$compiler = [Diagnostics.Process]::Start($start)
$compiler.WaitForExit()
if ($compiler.ExitCode -ne 0) { throw 'GPU helper compilation failed' }
# .NET apphost embeds the managed DLL name, so copies retain the shared DLL/runtimeconfig.
foreach ($name in 'cpu','memory','threads','handles','files','io-read','io-write','dotnet','gpu','objects') {
    Copy-Item -LiteralPath (Join-Path $out 'demo-workload.exe') -Destination (Join-Path $out "demo-$name.exe")
}
Write-Output "Built: $out"
