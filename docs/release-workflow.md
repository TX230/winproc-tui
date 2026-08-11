# Release Workflow

This document describes how to create a GitHub Release for `winproc-tui` and attach a Windows x64 binary package.

The examples below use `vX.Y.Z` as a placeholder for the release version (for example `v0.1.0`) and `TX230/winproc-tui` as the target repository.
Replace `vX.Y.Z` (and the numeric `X.Y.Z` form used in file names) with the actual release version each time; the procedure itself does not change between versions.

## Concepts

### Release Notes

This project does not maintain a separate `CHANGELOG.md` file.
Use `gh release create --generate-notes` to create draft release notes, then review and edit them before publishing.
GitHub's generated notes are a starting point, especially for releases built from merged maintainer-requested or AI-assisted pull requests; they are not a substitute for checking the actual commit range.

Published GitHub Release notes follow the [Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) conventions even though the repository does not maintain a separate changelog file. Under a short `What's changed` section, group notable user-visible outcomes by the applicable Keep a Changelog types, in this order: `Added`, `Changed`, `Deprecated`, `Removed`, `Fixed`, and `Security`. Omit empty types. The GitHub Release title and publication date provide the version and date metadata that a standalone changelog would normally place in its release heading.

Review the commits since the previous tag, but do not copy commit titles or Conventional Commit prefixes such as `feat:` and `fix:` mechanically. Write for users, group closely related commits into one entry when that makes the result easier to understand, and call out deprecations, removals, security fixes, and breaking changes explicitly. Append every relevant Issue number in the form `Issue #n`; when one entry covers several Issues, write each one explicitly. Do not attach an Issue number to unrelated release housekeeping.

Use this structure:

```markdown
## What's changed

### Added

- Describe a new user-visible capability. (Issue #n)

### Changed

- Describe a change to existing user-visible behavior. (Issue #n, Issue #n)

### Fixed

- Describe a user-visible bug fix. (Issue #n)

**Full Changelog**: https://github.com/TX230/winproc-tui/compare/<previous-tag>...<current-tag>
```

Generated notes may contain only a Full Changelog link when the release range has no merged pull requests. In that case, write the categorized `What's changed` entries manually. Prefer a notes file over a long inline shell argument so Markdown, backticks, and line breaks are not altered by PowerShell:

```powershell
gh release edit $Tag `
  --repo TX230/winproc-tui `
  --notes-file <release-notes-file>
```

### Git Tag

A Git tag is a stable name for a specific commit.

For example, the tag `vX.Y.Z` means:

```text
This exact source commit is winproc-tui vX.Y.Z.
```

The tag behaves like a source-code snapshot. Technically, it is a fixed label that points to a commit, not a separate copy of all files.

### GitHub Release

A GitHub Release is a distribution page attached to a tag.

It can contain:

- A release title.
- Release notes.
- GitHub-generated source archives.
- Manually uploaded assets such as `winproc-tui.exe` packaged in a `.zip` file.
- Checksum files such as `.sha256`.

The important rule is that the uploaded binary should be built from the same commit that the release tag points to.

## Publication Order

Publish in this order:

1. Publish and verify the versioned GitHub Release.
2. Update and verify `TX230/scoop-bucket`.
3. Submit the version to `microsoft/winget-pkgs`.

Both package-manager manifests must use the immutable version-specific Release URL and the SHA-256 of the published asset. Do not prepare them from a draft `untagged-*` asset URL, and do not replace a published asset after either manifest refers to it.

## Manual Release Procedure

Before starting, use Rust 1.95.0 or later and the C++ toolchain from Build Tools for Visual Studio 2026.

The commands below assume a PowerShell session in which the release version is set as shell variables.
Setting them once at the start lets every following command stay version-agnostic.
`$Version` stores the bare numeric version used inside file names; `$Tag` stores the `v`-prefixed Git tag.

```powershell
$Version  = "X.Y.Z"
$Tag      = "v$Version"
$ZipName  = "winproc-tui-$Version-windows-x64.zip"
$ZipPath  = "dist\$ZipName"
$Sha256   = "$ZipPath.sha256"
```

Replace `X.Y.Z` with the actual release version (for example `0.1.0`).

### Packaging Helper Script

The repository also provides a helper script for the test, build, zip, and checksum steps:

```powershell
.\scripts\package-release.ps1 -Version 0.1.0
```

If `-Version` is omitted, the script uses the package version from `Cargo.toml`.
The script creates `dist\winproc-tui-X.Y.Z-windows-x64.zip` and `dist\winproc-tui-X.Y.Z-windows-x64.zip.sha256`.
Before packaging, it verifies that the executable does not dynamically import Microsoft C runtime DLLs.
Tag creation and GitHub Release creation remain explicit manual steps so that the maintainer can confirm the exact source commit and draft release contents before publishing.

### 1. Confirm the Target Repository

Check the current remote:

```powershell
git remote -v
```

The remote should point to:

```text
https://github.com/TX230/winproc-tui.git
```

The release command also uses `--repo TX230/winproc-tui` so the target repository is explicit.

### 2. Confirm GitHub CLI Authentication

```powershell
gh auth status
```

If authentication is missing, sign in before continuing:

```powershell
gh auth login
```

### 3. Confirm the Workspace State

```powershell
git status
```

The release tag should be created from the commit that is intended to become the release.
If there are uncommitted changes, either commit them first or intentionally leave them out of the release.

### 4. Run Tests

```powershell
cargo test
```

If the normal target directory is blocked because an executable is locked, use a separate target directory:

```powershell
$env:CARGO_TARGET_DIR = "target/codex-build"
cargo test
```

### 5. Build the Release Binary

```powershell
cargo build --release
```

`.cargo/config.toml` enables `+crt-static` for `x86_64-pc-windows-msvc`, so the Release executable does not require a separately installed Microsoft Visual C++ Redistributable.

The executable is generated at:

```text
target\release\winproc-tui.exe
```

### 6. Create the Distribution Package

After completing the test and build steps manually, use the packaging helper without rerunning them:

```powershell
.\scripts\package-release.ps1 -Version $Version -SkipTests -SkipBuild
```

The release archive is intentionally runtime-only. Documentation remains on GitHub and is not copied into the zip. The zip contains:

```text
winproc-tui.exe
LICENSE
```

The packaging helper rejects the executable if `dumpbin /dependents` reports dynamic Microsoft C runtime imports such as `VCRUNTIME140.dll` or `api-ms-win-crt-*.dll`.

The package name includes:

- Project name: `winproc-tui`
- Version: the value of `$Version` without the `v` prefix (for example `0.1.0`)
- Platform: `windows-x64`

`LICENSE` is a distribution notice rather than product documentation and remains beside the binary. `README.md`, `README.ja.md`, `assets/`, and `docs/` are not packaged.

`winproc-tui.toml` is also not prepackaged. It is user-specific session state: the application starts with defaults when the file is absent, then creates or updates it next to `winproc-tui.exe` after a successful run. The helper stops with an error unless the archive contains exactly the executable and `LICENSE`.

### 7. Create a Checksum File

```powershell
Get-FileHash $ZipPath -Algorithm SHA256 |
  ForEach-Object { "$($_.Hash)  $ZipName" } |
  Set-Content $Sha256
```

GitHub also computes and displays a `sha256:` digest for each uploaded release asset. The `.sha256` file remains useful as a downloadable checksum for command-line or scripted checks.
Before upload, compare the generated checksum against the package you are about to attach:

```powershell
Get-FileHash $ZipPath -Algorithm SHA256
Get-Content $Sha256
```

The hash values should match.

### 8. Create and Push the Git Tag

```powershell
git tag -a $Tag -m "winproc-tui $Tag"
git push origin $Tag
```

Confirm the tag:

```powershell
git show $Tag --stat
```

If this is not the first release, also review the commit range from the previous tag:

```powershell
git log <previous-tag>..$Tag --oneline
```

### 9. Create a Draft GitHub Release

```powershell
gh release create $Tag `
  $ZipPath `
  $Sha256 `
  --repo TX230/winproc-tui `
  --title "winproc-tui $Tag" `
  --generate-notes `
  --draft
```

Command meaning:

- `gh release create $Tag`: Create a release for the version tag.
- `$ZipPath`: Upload the binary package as a release asset.
- `$Sha256`: Upload the checksum file as a release asset.
- `--repo TX230/winproc-tui`: Specify the target repository explicitly.
- `--title "winproc-tui $Tag"`: Set the visible release title.
- `--generate-notes`: Ask GitHub to generate draft release notes. This project does not maintain a separate `CHANGELOG.md`, so review the generated notes against the commit range before publishing.
- `--draft`: Create the release as a draft so it can be reviewed before publishing.

### 10. Review Before Publishing

Open the draft release in GitHub and confirm:

- The release points to the intended tag.
- The release title is correct.
- The generated notes match the intended release contents. Edit them in the draft if any entry is missing, unclear, or duplicated; the edited text becomes the final published release notes.
- The notes contain a `What's changed` section, every relevant `Issue #n`, and a Full Changelog link for the tag range.
- The `.zip` and `.sha256` files are attached.
- The attached `.sha256` file matches the attached `.zip` file.
- The GitHub-displayed `sha256:` digest for the `.zip` asset matches the generated checksum.
- The `.zip` file contains exactly `winproc-tui.exe` and `LICENSE`.
- The `.zip` file does not contain README files, `assets/`, `docs/`, or a preset `winproc-tui.toml`.
- A clean extraction starts with default settings; after a successful run, `winproc-tui.toml` is created next to the executable.
- The release page does not point users to third-party binaries or mirrors as official builds.
- The executable starts successfully on Windows 11 x64.

Inspect the draft from the command line as well:

```powershell
gh release view $Tag `
  --repo TX230/winproc-tui `
  --json name,tagName,targetCommitish,isDraft,isPrerelease,body,assets
```

A draft asset URL may contain `releases/download/untagged-*`. That is expected while the release is a draft; verify `tagName`, the target commit, asset names, sizes, and digests instead of treating the draft URL as a publication failure.

After confirming the draft, publish it from the GitHub Releases page or with:

```powershell
gh release edit $Tag `
  --repo TX230/winproc-tui `
  --draft=false `
  --latest
```

Run `gh release view` again after publication. Confirm that `isDraft` is `false`, the public asset URL contains `/releases/download/$Tag/`, and the GitHub asset digest matches the local SHA-256 before updating Scoop or winget.

## Scoop Bucket Publication

The custom Scoop bucket is published from `TX230/scoop-bucket`. Its `bucket/winproc-tui.json` manifest downloads the versioned Windows x64 zip directly from this repository's GitHub Release, verifies its SHA-256 hash, registers the `winproc-tui` command, and persists `winproc-tui.toml`.

### Update the Custom Bucket Manifest

Publish and verify the GitHub Release before updating the Scoop manifest. Never use a `latest` asset URL. Set the manifest `version`, version-specific `url`, and `hash` from the published asset, and do not replace an asset after a manifest refers to its URL.

Clone the bucket or fast-forward an existing clean clone:

```powershell
gh repo clone TX230/scoop-bucket <scoop-bucket>
Set-Location <scoop-bucket>

# For an existing clone:
git switch master
git pull --ff-only origin master
```

Update only the release-specific values in `bucket/winproc-tui.json`:

- `version`: the numeric version without the `v` prefix.
- `architecture.64bit.url`: `https://github.com/TX230/winproc-tui/releases/download/vX.Y.Z/winproc-tui-X.Y.Z-windows-x64.zip`.
- `architecture.64bit.hash`: the lowercase SHA-256 of that published zip.

Keep `bin`, `pre_install`, `persist`, `checkver`, and `autoupdate` intact unless their behavior actually needs to change. Confirm that the JSON parses and inspect the resolved values:

```powershell
$Manifest = Get-Content bucket\winproc-tui.json -Raw | ConvertFrom-Json
$Manifest.version
$Manifest.architecture.'64bit'.url
$Manifest.architecture.'64bit'.hash
```

The manifest keeps the release zip unchanged. It uses `pre_install` to create an empty `winproc-tui.toml` only when no persisted file exists, then declares the file in `persist`. Do not add a preset config to the release zip.

### Validate the Scoop Lifecycle

After updating the manifest, confirm that Scoop detects the intended release:

```powershell
& "$(scoop prefix scoop)\bin\checkver.ps1" `
  -App winproc-tui `
  -Dir <scoop-bucket>\bucket `
  -ThrowError
```

Run the bucket tests and perform a clean lifecycle check:

```powershell
scoop bucket add tx230 https://github.com/TX230/scoop-bucket
scoop install tx230/winproc-tui
& "$(scoop prefix winproc-tui)\winproc-tui.exe" --version
scoop uninstall winproc-tui
scoop install tx230/winproc-tui
scoop uninstall --purge winproc-tui
```

Confirm at least:

- The manifest passes the Scoop schema and bucket tests.
- The downloaded zip passes its SHA-256 check.
- The installed executable reports the intended version.
- The shim is created and removed correctly.
- A normal uninstall preserves `winproc-tui.toml`.
- Reinstallation reuses the persisted config.
- `--purge` removes the persisted config.
- `checkver` and `autoupdate` resolve the versioned Release asset naming convention.

Prefer a clean Windows Sandbox for the lifecycle check when the host already has `winproc-tui` installed. This avoids replacing the maintainer's current installation or persisted config. In the Sandbox, install from the local manifest before publishing, then repeat a remote-bucket install after the bucket commit is public when practical.

If the `tx230` bucket is already registered on a machine, refresh all bucket manifests before checking for an application update:

```powershell
scoop update
scoop update winproc-tui
```

`scoop update tx230/winproc-tui` alone does not refresh a stale local bucket checkout first.

### Publish and Verify the Bucket

Commit only `bucket/winproc-tui.json`, include a concise commit body, and push the bucket's `master` branch:

```powershell
git add bucket\winproc-tui.json
git commit `
  -m "chore(winproc-tui): update to X.Y.Z" `
  -m "Point the Scoop manifest at the vX.Y.Z release asset and update its SHA-256 hash."
git push origin master
```

Wait for the bucket CI to pass in both Windows PowerShell and PowerShell 7:

```powershell
$RunId = gh run list `
  --repo TX230/scoop-bucket `
  --branch master `
  --workflow CI `
  --limit 1 `
  --json databaseId `
  --jq '.[0].databaseId'

gh run watch $RunId `
  --repo TX230/scoop-bucket `
  --exit-status
```

Do not report the version as published through Scoop until the remote manifest points to the intended immutable Release asset and the CI succeeds.

## Windows Package Manager Publication

The winget package identifier is `TX230.winproc-tui`. Its portable manifest downloads the versioned Windows x64 zip directly from this repository's GitHub Release and registers the `winproc-tui` command.

Publish the GitHub Release before submitting its winget manifest. Never use a `latest` download URL: the manifest must point to the version-specific asset URL and contain the SHA-256 of that exact asset. Once a manifest refers to a published asset, do not replace the asset at the same URL. Publish a new version instead.

### Prepare the Fork and Sparse Checkout

Synchronize the fork's `master` branch with `microsoft/winget-pkgs`, then create a fresh sparse checkout and a version-specific branch:

```powershell
gh repo sync TX230/winget-pkgs `
  --source microsoft/winget-pkgs `
  --branch master

git clone `
  --filter=blob:none `
  --sparse `
  https://github.com/TX230/winget-pkgs.git `
  <winget-pkgs>

Set-Location <winget-pkgs>
git sparse-checkout set manifests/t/TX230/winproc-tui Tools
git switch -c update-tx230-winproc-tui-X.Y.Z
```

Do not reuse a dirty checkout from an earlier Sandbox test. `SandboxTest.ps1` may leave generated or modified validation files, and unrelated changes must not enter the manifest commit.

### Create the Version Manifests

Create one directory under `manifests/t/TX230/winproc-tui/X.Y.Z/` containing exactly these four files:

- `TX230.winproc-tui.yaml`: version manifest.
- `TX230.winproc-tui.installer.yaml`: portable zip installer manifest.
- `TX230.winproc-tui.locale.en-US.yaml`: default English locale.
- `TX230.winproc-tui.locale.ja-JP.yaml`: Japanese locale.

Use the schema version currently recommended by the `microsoft/winget-pkgs` pull request template. The installer manifest uses:

```yaml
InstallerType: zip
NestedInstallerType: portable
Commands:
- winproc-tui
Installers:
- Architecture: x64
  NestedInstallerFiles:
  - RelativeFilePath: winproc-tui.exe
    PortableCommandAlias: winproc-tui
```

Do not set `Scope` in the manifest; winget does not support that field for a portable installer. The default portable installation is per-user.

For every version, update and review at least:

- `PackageVersion` in all four files.
- `InstallerUrl`, `InstallerSha256`, and `ReleaseDate` in the installer manifest.
- Version-tagged `LicenseUrl`, `ReleaseNotesUrl`, and documentation URLs.
- The locale descriptions and tags when product behavior changed.

After the first version is available in the winget catalog, WinGetCreate can generate an update candidate:

```powershell
$ReleaseZipUrl = "https://github.com/TX230/winproc-tui/releases/download/$Tag/$ZipName"
wingetcreate update TX230.winproc-tui `
  --urls $ReleaseZipUrl `
  --version $Version `
  --release-notes-url "https://github.com/TX230/winproc-tui/releases/tag/$Tag" `
  --release-date (Get-Date -Format yyyy-MM-dd) `
  --out <output-directory>
```

If WinGetCreate reports that it cannot parse the portable Release zip, copy the previous version's four manifests into the new version directory and update them manually. Review every URL and hash; do not change the portable zip structure merely to satisfy WinGetCreate.

### Validate in Windows Sandbox

Validate and test the manifest before submission:

```powershell
winget validate --manifest <manifest-directory>
.\Tools\SandboxTest.ps1 <manifest-directory>
```

`SandboxTest.ps1` is provided by the `microsoft/winget-pkgs` repository. The host `winget.exe` may fail to start in a non-interactive or stale logon session; that host-session error is not evidence that the manifest failed. Use the official Sandbox path for the authoritative local test. Close any existing Windows Sandbox instance before starting another configuration.

For Sandbox automation:

- Add `--accept-source-agreements` to commands that access a catalog source.
- Add `--source winget` when an exact catalog install would otherwise match more than one source.
- Keep install, upgrade, and uninstall in machine scope when the official Sandbox test is running in an administrator context. Pass `--scope machine` to those test commands; do not add `Scope` to the portable manifest.
- Use Windows PowerShell 5.1-compatible `Set-Content -Encoding utf8` in helper scripts. `utf8NoBOM` is not supported there.
- Use the same package source identity on both sides of an upgrade test. Install the previous and current versions from two local manifests, or install both from the public catalog. Do not install the previous version from the catalog and upgrade it from a local manifest when testing config preservation, because that changes the package directory identity.
- Place the config sentinel next to the actual installed package executable, not next to the command alias under `WinGet\Links`. Resolve the alias target or locate `winproc-tui.exe` under the winget package directory before creating `winproc-tui.toml`.

Confirm at least:

- The manifest validates without warnings.
- The published zip downloads and passes its SHA-256 check.
- The package installs in the intended scope.
- The default per-user install works in a non-elevated session when that test environment is available.
- The `winproc-tui` command is registered and the application starts.
- Upgrading from the previous version preserves `winproc-tui.toml`.
- A clean install reports the intended version.
- Uninstall succeeds and removes the command alias.

### Commit and Submit the Pull Request

Check that there is no other open pull request for the same package version, then commit only the new version directory with a concise body:

```powershell
gh pr list `
  --repo microsoft/winget-pkgs `
  --state open `
  --search 'TX230.winproc-tui in:title'

git add manifests/t/TX230/winproc-tui/X.Y.Z
git commit `
  -m "chore: update TX230.winproc-tui to X.Y.Z" `
  -m "Add portable ZIP manifests for the statically linked vX.Y.Z release."
git push -u origin update-tx230-winproc-tui-X.Y.Z
```

Fill out the current upstream pull request template and mark the CLA, duplicate-PR check, single-manifest scope, local validation, local install test, and schema checks accurately. Submit a ready pull request with the required title format:

```powershell
gh pr create `
  --repo microsoft/winget-pkgs `
  --base master `
  --head TX230:update-tx230-winproc-tui-X.Y.Z `
  --title "Update: TX230.winproc-tui to X.Y.Z" `
  --body-file <pull-request-body-file>
```

After submission, distinguish these states clearly:

- CLA success confirms only the contributor agreement.
- The winget validation pipeline is a separate automated check.
- An open, non-draft, mergeable pull request can still be blocked pending Microsoft moderator review.
- The package is not available from the public winget source until the pull request is merged and the catalog is updated.

Update README installation instructions only after the first manifest is available from the public winget source.

### Withdraw a Defective Submission

If a release defect is discovered before a winget pull request is merged, leave a concise explanation on the pull request and close it:

```powershell
gh pr comment <pull-request-number> `
  --repo microsoft/winget-pkgs `
  --body <withdrawal-reason>

gh pr close <pull-request-number> `
  --repo microsoft/winget-pkgs
```

Do not replace the defective asset at the existing Release URL. Publish a corrected version with a new immutable URL and submit a new one-version pull request.
