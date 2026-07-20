# Release Workflow

This document describes how to create a GitHub Release for `winproc-tui` and attach a Windows x64 binary package.

The examples below use `vX.Y.Z` as a placeholder for the release version (for example `v0.1.0`) and `TX230/winproc-tui` as the target repository.
Replace `vX.Y.Z` (and the numeric `X.Y.Z` form used in file names) with the actual release version each time; the procedure itself does not change between versions.

## Concepts

### Release Notes

This project does not maintain a separate `CHANGELOG.md` file.
Use `gh release create --generate-notes` to create draft release notes, then review and edit them before publishing.
GitHub's generated notes are a starting point, especially for releases built from merged maintainer-requested or AI-assisted pull requests; they are not a substitute for checking the actual commit range.

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

The executable is generated at:

```text
target\release\winproc-tui.exe
```

### 6. Create the Distribution Package

After completing the test and build steps manually, use the packaging helper without rerunning them:

```powershell
.\scripts\package-release.ps1 -Version $Version -SkipTests -SkipBuild
```

The helper preserves the repository-relative paths needed by links in the packaged README files. The zip contains:

```text
winproc-tui.exe
README.md
README.ja.md
LICENSE
assets/
docs/
```

The package name includes:

- Project name: `winproc-tui`
- Version: the value of `$Version` without the `v` prefix (for example `0.1.0`)
- Platform: `windows-x64`

`LICENSE` is included so that the MIT license terms travel with the binary distribution. `assets/` and `docs/` are included so local image and documentation links in both README files continue to work after extraction. The helper stops with an error if either packaged README contains a local link whose target is missing from the zip.

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
- The `.zip` and `.sha256` files are attached.
- The attached `.sha256` file matches the attached `.zip` file.
- The GitHub-displayed `sha256:` digest for the `.zip` asset matches the generated checksum.
- The `.zip` file contains the expected executable, README files, `LICENSE`, `assets/`, and `docs/`.
- Relative image and documentation links in both packaged README files resolve inside the extracted package.
- The release page does not point users to third-party binaries or mirrors as official builds.
- The executable starts successfully on Windows 11 x64.

After confirming the draft, publish it from the GitHub Releases page.
