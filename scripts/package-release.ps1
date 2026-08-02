param(
    [string]$Version,
    [switch]$SkipTests,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")

function Get-CargoVersion {
    $cargoToml = Join-Path $RepoRoot "Cargo.toml"
    $versionLine = Select-String -Path $cargoToml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if (-not $versionLine) {
        throw "Could not find package version in Cargo.toml."
    }

    return $versionLine.Matches[0].Groups[1].Value
}

function Invoke-CheckedNativeCommand {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Command,

        [Parameter(ValueFromRemainingArguments = $true)]
        [string[]]$Arguments
    )

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed with exit code ${LASTEXITCODE}: $Command $($Arguments -join ' ')"
    }
}

function Get-DumpbinPath {
    $dumpbinCommand = Get-Command "dumpbin.exe" -ErrorAction SilentlyContinue
    if ($dumpbinCommand) {
        return $dumpbinCommand.Source
    }

    $vswherePath = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswherePath) {
        $dumpbinPaths = @(
            & $vswherePath `
                -latest `
                -products * `
                -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
                -find "VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe"
        )
        if ($LASTEXITCODE -eq 0 -and $dumpbinPaths.Count -gt 0) {
            return $dumpbinPaths[0]
        }
    }

    throw "dumpbin.exe was not found. Install the Visual C++ x64 build tools before packaging."
}

function Assert-StaticMsvcRuntime {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ExecutablePath
    )

    $dumpbinPath = Get-DumpbinPath
    $dumpbinOutput = @(& $dumpbinPath /nologo /dependents $ExecutablePath 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "dumpbin.exe failed with exit code ${LASTEXITCODE}: $ExecutablePath"
    }

    $dynamicRuntimePattern = '(?i)\b(?:vcruntime\d+(?:_\d+)?|msvcp\d+(?:_\d+)?|msvcr\d+|ucrtbase|api-ms-win-crt-[a-z0-9-]+)\.dll\b'
    $dynamicRuntimeDlls = @(
        [regex]::Matches(($dumpbinOutput -join "`n"), $dynamicRuntimePattern) |
            ForEach-Object { $_.Value } |
            Sort-Object -Unique
    )

    if ($dynamicRuntimeDlls.Count -gt 0) {
        throw "Release executable dynamically links the Microsoft C runtime: $($dynamicRuntimeDlls -join ', ')"
    }
}

function Assert-PackageEntries {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArchivePath,

        [Parameter(Mandatory = $true)]
        [string[]]$ExpectedEntries
    )

    $archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
    try {
        $actualEntries = @(
            $archive.Entries |
                Where-Object { -not $_.FullName.EndsWith("/") } |
                ForEach-Object { $_.FullName.Replace("\", "/") } |
                Sort-Object -Unique
        )
        $normalizedExpectedEntries = @(
            $ExpectedEntries |
                ForEach-Object { $_.Replace("\", "/") } |
                Sort-Object -Unique
        )
        $missingEntries = @(
            $normalizedExpectedEntries | Where-Object { $actualEntries -notcontains $_ }
        )
        $unexpectedEntries = @(
            $actualEntries | Where-Object { $normalizedExpectedEntries -notcontains $_ }
        )

        if ($missingEntries.Count -gt 0 -or $unexpectedEntries.Count -gt 0) {
            $details = @()
            if ($missingEntries.Count -gt 0) {
                $details += "Missing entries: $($missingEntries -join ', ')"
            }
            if ($unexpectedEntries.Count -gt 0) {
                $details += "Unexpected entries: $($unexpectedEntries -join ', ')"
            }
            throw "Release package contents do not match the runtime-only policy:`n$($details -join "`n")"
        }
    }
    finally {
        $archive.Dispose()
    }
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = Get-CargoVersion
}

$ZipName = "winproc-tui-$Version-windows-x64.zip"
$ZipPath = Join-Path $RepoRoot "dist\$ZipName"
$Sha256Path = "$ZipPath.sha256"
$ExePath = Join-Path $RepoRoot "target\release\winproc-tui.exe"
$PackageEntries = @(
    [pscustomobject]@{ Source = $ExePath; Destination = "winproc-tui.exe" }
    [pscustomobject]@{ Source = (Join-Path $RepoRoot "LICENSE"); Destination = "LICENSE" }
)

# winproc-tui.toml is user-specific session state. The application creates or
# updates it next to the executable after a successful run, so no preset config
# is included in the release archive.

Push-Location $RepoRoot
try {
    if (-not $SkipTests) {
        Invoke-CheckedNativeCommand cargo test
    }

    if (-not $SkipBuild) {
        Invoke-CheckedNativeCommand cargo build --release
    }

    if (-not (Test-Path $ExePath)) {
        throw "Release executable was not found: $ExePath"
    }

    Assert-StaticMsvcRuntime -ExecutablePath $ExePath

    New-Item -ItemType Directory -Force (Join-Path $RepoRoot "dist") | Out-Null

    if (Test-Path $ZipPath) {
        Remove-Item -LiteralPath $ZipPath -Force
    }

    $archive = [System.IO.Compression.ZipFile]::Open(
        $ZipPath,
        [System.IO.Compression.ZipArchiveMode]::Create
    )
    try {
        foreach ($entry in $PackageEntries) {
            [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
                $archive,
                $entry.Source,
                $entry.Destination,
                [System.IO.Compression.CompressionLevel]::Optimal
            ) | Out-Null
        }
    }
    finally {
        $archive.Dispose()
    }

    Assert-PackageEntries `
        -ArchivePath $ZipPath `
        -ExpectedEntries @($PackageEntries.Destination)

    $hash = Get-FileHash $ZipPath -Algorithm SHA256
    $checksumText = "$($hash.Hash)  $ZipName`n"
    [System.IO.File]::WriteAllText($Sha256Path, $checksumText, [System.Text.UTF8Encoding]::new($false))

    Write-Host "Created package: $ZipPath"
    Write-Host "Created checksum: $Sha256Path"
    Write-Host "After publishing the GitHub Release, update the Scoop manifest:"
    Write-Host "  Bucket: https://github.com/TX230/scoop-bucket"
    Write-Host "  Version: $Version"
    Write-Host "  URL: https://github.com/TX230/winproc-tui/releases/download/v$Version/$ZipName"
    Write-Host "  SHA256: $($hash.Hash.ToLowerInvariant())"
}
finally {
    Pop-Location
}
