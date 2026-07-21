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
}
finally {
    Pop-Location
}
