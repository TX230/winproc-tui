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

function Assert-PackagedReadmeLinks {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArchivePath
    )

    $archive = [System.IO.Compression.ZipFile]::OpenRead($ArchivePath)
    try {
        $entryNames = @($archive.Entries | ForEach-Object { $_.FullName })
        $brokenLinks = @()

        foreach ($readmeName in @("README.md", "README.ja.md")) {
            $readmeEntry = $archive.GetEntry($readmeName)
            if (-not $readmeEntry) {
                throw "Packaged README was not found: $readmeName"
            }

            $reader = [System.IO.StreamReader]::new($readmeEntry.Open())
            try {
                $content = $reader.ReadToEnd()
            }
            finally {
                $reader.Dispose()
            }

            foreach ($match in [regex]::Matches($content, '\]\(([^)]+)\)')) {
                $target = $match.Groups[1].Value.Split("#")[0]
                if (-not $target -or $target -match '^(https?://|mailto:)') {
                    continue
                }

                $normalizedTarget = $target.Replace("\", "/")
                if ($entryNames -notcontains $normalizedTarget) {
                    $brokenLinks += "$readmeName -> $target"
                }
            }
        }

        if ($brokenLinks.Count -gt 0) {
            throw "Packaged README contains broken local links:`n$($brokenLinks -join "`n")"
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
    [pscustomobject]@{ Source = (Join-Path $RepoRoot "README.md"); Destination = "README.md" }
    [pscustomobject]@{ Source = (Join-Path $RepoRoot "README.ja.md"); Destination = "README.ja.md" }
    [pscustomobject]@{ Source = (Join-Path $RepoRoot "LICENSE"); Destination = "LICENSE" }
)

foreach ($relativeDirectory in @("assets", "docs")) {
    $directory = Join-Path $RepoRoot $relativeDirectory
    $PackageEntries += Get-ChildItem -LiteralPath $directory -Recurse -File | ForEach-Object {
        [pscustomobject]@{
            Source = $_.FullName
            Destination = [System.IO.Path]::GetRelativePath($RepoRoot, $_.FullName).Replace("\", "/")
        }
    }
}

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

    Assert-PackagedReadmeLinks -ArchivePath $ZipPath

    $hash = Get-FileHash $ZipPath -Algorithm SHA256
    $checksumText = "$($hash.Hash)  $ZipName`n"
    [System.IO.File]::WriteAllText($Sha256Path, $checksumText, [System.Text.UTF8Encoding]::new($false))

    Write-Host "Created package: $ZipPath"
    Write-Host "Created checksum: $Sha256Path"
}
finally {
    Pop-Location
}
