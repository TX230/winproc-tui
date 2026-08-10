# AGENTS.md

This document describes operating rules for AI coding agents (Codex, Cursor, and similar tools) working in this repository. The maintainer does not need to follow it as a checklist; durable product rules live in `README.md`, `README.ja.md`, and the documents under `docs/`.

This repository is the development repository for `winproc-tui`.
`winproc-tui` is a TUI process investigation tool for Windows 11 x64. It uses Rust 2024 edition, ratatui, crossterm, Windows APIs, PDH, DXGI, and sysinfo.

## Read Before Working

This repository has specifications under `docs/`. Before changing implementation or explanations, read the documents relevant to the requested work.

- `docs/metrics.md`: Metrics, data sources, display formats, CPU% semantics, sampling frequency, and recording logs.
- `docs/architecture.md`: Responsibility boundaries, runtime data flow, design decisions, major state, invariants, and testing approach.
- `docs/release-workflow.md`: Release tagging, packaging, and GitHub Release procedure.
- `README.ja.md`: Japanese user-facing overview.
- `README.md`: English user-facing overview for GitHub. Keep it synchronized with `README.ja.md`.

Prefer the current implementation under `src/` over old notes or guesses.
If the specifications and implementation conflict, inspect the implementation first and update the specifications if needed.

## Repository Policy

- Store text files as UTF-8 without BOM, using LF line endings. Keep this aligned with `.gitattributes`.
- This project is Windows-only. Do not add abstractions or explanations that assume Linux / macOS support unless explicitly requested.
- This is a personal project. Unsolicited pull requests from external contributors are not accepted. Use GitHub Issues for feedback and feature requests.
- `docs/` is Git-managed primary information for specifications, architecture, metrics, and release workflow. When implementation or specifications change, update the related documents in the same work item.
- `logs/` and `notes/` are local-only paths ignored by `.gitignore`. Do not treat them as publishable artifacts unless the user explicitly says so.
- Local-only work that changes only ignored paths such as `notes/` or `logs/` does not require an agent branch or commit.
- Existing uncommitted changes may be user work. Do not revert changes you did not make.
- Keep changes as small as practical. Avoid opportunistic large refactors and unrelated formatting churn.
- Keep maintained specifications under `docs/` in English.
- Keep Japanese documentation limited to `README.ja.md` unless the user explicitly asks otherwise.
- In `README.ja.md`, prefer natural, readable Japanese over literal translation or unnecessary English mixing.
- The GitHub Release zip is runtime-only. Package `winproc-tui.exe` and the `LICENSE` distribution notice, but do not package README files, `assets/`, `docs/`, or a preset `winproc-tui.toml`. The application creates or updates its user-specific config next to the executable after a successful run.
- Release builds for `x86_64-pc-windows-msvc` must statically link the Microsoft C runtime. Do not package or publish an executable that imports runtime DLLs such as `VCRUNTIME140.dll` or `api-ms-win-crt-*.dll`.

## Documentation Workflow

- In general, work on the change requested by the user. If the user selects a GitHub Issue, work on exactly that one issue.
- Before implementing, read the target issue or request and related specifications. Do not mix requirements, design, and implementation instructions.
- If metrics, data sources, display formats, or recording log values change, update `docs/metrics.md`.
- If responsibility boundaries, runtime data flow, design decisions, major state, invariants, or the testing approach change, update `docs/architecture.md`.
- If user-facing behavior changes, update Help, Footer, README, tests, and source as appropriate.
- Keep exact key lists, colors, emphasis, cell widths, marker shapes, and drawing positions in implementation and tests rather than expanding `docs/architecture.md` with rendering details.
- If release contents, packaging checks, tagging, or publishing steps change, update `scripts/package-release.ps1` and `docs/release-workflow.md` together.
- After implementation, perform a documentation-impact check and update only the canonical owners affected by the change in the same work item.
- If a technical choice needs durable context, keep it in the related specification, architecture document, or GitHub Issue.
- Do not create or update repository-local backlog files under `docs/backlog/`; use GitHub Issues for backlog tracking.

## Commit Rules

- Use English Conventional Commits for commit messages.
- Every commit must include a concise commit message body that summarizes the changes so readers can understand what changed when reviewing the commit history.
- Keep commits scoped. Do not include unrelated dirty files or local-only artifacts.
- When a coherent unit of AI work is complete, commit it promptly.
- Do not commit ignored local-only files such as `notes/` or `logs/` unless the user explicitly asks to track them.
- When committing implementation work, include updates to the affected canonical documentation in the same commit. Do not change `docs/architecture.md` mechanically when its design-level content is unaffected.
- Reference the relevant GitHub Issue in the commit message or maintainer-requested pull request when useful.
- Disambiguate GitHub item numbers in human-facing text and commit titles: write `Issue #n` for Issues and `PR #n` for pull requests. Avoid a bare `#n` except where GitHub syntax requires it, such as `Closes #n` or `Refs #n`.

## Branch Workflow Rules

These branch / commit / push rules apply to AI agents. The maintainer usually integrates work locally; open a pull request only when the maintainer explicitly asks for one.

- Treat `main` as the stable default branch. Do not use it for experiments or multi-step work.
- AI agents must work on an `agent/<short-topic>` branch for tracked repository changes.
- Prefer a branch name that describes the work, for example `agent/help-dialog-copy` or `agent/branch-workflow-docs`.
- If the human gives a branch name, use the human-specified name instead of inventing one.
- Use `agent/YYYYMMDD-HHMM` only as a fallback when there is no clear topic name or when the human explicitly asks for a timestamp-only branch.
- AI agents must not commit to `main` unless the user explicitly instructs them to do so.
- Create the agent branch from the current working branch unless the user explicitly asks to start from another branch.
- If the task only creates or updates ignored local-only files such as `notes/` or `logs/`, stay on the current branch and do not create an agent branch.
- Humans may review one or more AI commits together, ask for fixes on the same agent branch, then squash merge to `main` with one English summary commit.
- Delete the agent branch immediately after its work has been squash merged to `main`, or when the user decides to discard it.
- Do not force-push or rewrite published `main`.
- AI agents must not push `main` unless the user explicitly asks to push.

## Main Integration Rules

These rules apply when the user asks an AI agent to integrate a completed agent branch into `main`.

- Before integrating an agent branch into `main`, ask the user whether there is a related GitHub Issue number.
- Prefer squash-merging completed agent branch work into `main` as one coherent English Conventional Commit.
- When the squash merge corresponds to a GitHub Issue, append the issue number to the commit title as `(Issue #n)`, for example `fix: place graph a/b labels on x-axis (Issue #3)`, so `git log --oneline` remains easy to scan without confusing the Issue number with a PR number.
- If the work completes a GitHub Issue, include `Closes #n` in the commit body. Use `Refs #n` instead if the Issue should remain open.
- A typical local integration sequence is:

```powershell
git switch main
git merge --squash agent/<short-topic>
git commit -m "<message> (Issue #n)" -m "Closes #n"
```

- Pushing `main` is normally performed by the user. AI agents must not run `git push origin main` unless the user explicitly asks them to push.

## Issue Workflow Rules

- GitHub Issues are the backlog and status-management surface.
- Write GitHub Issue titles and bodies in English.
- Use only two issue types: Bug report and Feature request.
- Issue templates are intentionally light. At minimum, a goal or a description of what is broken is required. Background, scope, acceptance criteria, and test plan are optional and can be added when implementation actually starts or in related commits on the agent branch.
- Keep durable product behavior in Help, Footer, README, tests, source, and this file when it is an agent-facing invariant. Keep metric definitions in `docs/metrics.md` and architecture in `docs/architecture.md`.
- Issue discussion, triage, labels, and status changes do not require repository commits by themselves.
- Do not reintroduce `docs/backlog/index.md` or `docs/backlog/BL-xxx.md` unless the user explicitly reverses this policy.

## GitHub CLI Authentication

- A failed `gh auth status` inside the Codex Sandbox does not prove that the maintainer's GitHub CLI credentials are invalid. The Sandbox user may be unable to access credentials stored for the host user in Windows Credential Manager.
- Do not ask the maintainer to run `gh auth login` based only on an authentication failure inside the Sandbox.
- When authenticated `gh` access is required, verify it outside the Sandbox with `gh auth status --hostname github.com` and run the necessary `gh` command outside the Sandbox so it can reuse the host user's existing credentials.
- Ask the maintainer to log in again only when the host-side authentication check also reports that authentication is missing or invalid.
- Do not copy a token from the host keyring into repository files, command output, or plaintext configuration as a workaround for Sandbox isolation.

## Implementation Guide

- Keep `model` as a data layer that does not depend on UI or samplers.
- Prefer keeping sampling non-blocking for the UI. Do not place heavy collection work on the UI thread.
- When adding a metric, check at least `model::columns`, `model::snapshot` / `process`, `samplers`, `ui::format`, display tables, Details, and recording logs.
- `CPU%` is a percentage of total logical CPU capacity. Read PDH `\Process(*)\% Processor Time` with `PDH_FMT_NOCAP100`, then divide by the logical CPU count.
- Unavailable values should generally be displayed as `--` in the UI and omitted from recording logs rather than written as `null`.
- The config file is `winproc-tui.toml`. It saves session state on exit and restores it on the next launch.
- Do not save Filter input state to the config file.
- Treat `tracked_only` as an independent state. Do not infer it from whether the tracked list is non-empty.
- Treat the working Tracked List and saved named Tracked Lists as separate state. `Space` changes only the working list; saved definitions change only through explicit Save, Save As, Rename, or Delete actions.
- When startup mode requires a Tracked List choice, resolve it before the initial sample so tracked-history retention applies from the first capture.

## User-Facing Behavior Rules

- The app has three user-visible activities: `Live`, `Recording`, and `Log view`.
- `Live` displays live snapshots from the sampling worker.
- `Recording` displays live snapshots and appends them to a JSON Lines recording session.
- `Log view` shows the last process snapshot and recorded metric histories from a saved log; it does not play frames over time.
- The header labels these activities as `LIVE`, `REC`, and `LOG`.
- Live and Recording show no normal freshness text. At 3 seconds without a successfully applied sample, the header adds `STALE Ns` until another sample succeeds.
- `DISPLAY PAUSED` freezes only the displayed snapshot. Sampling and Recording continue, and display pause is unavailable in Log view.
- `Recording` and `Log view` are mutually exclusive.
- Starting recording requires at least one configured Tracked List entry.
- Recording may start even when no configured tracked name currently matches a live process; frames still record system metrics and use an empty `processes` array until a matching process appears.
- Recording is unavailable in Log view, and Log view is unavailable during Recording.
- Stopping recording must flush and close the recording log.
- Quitting during recording must flush the recording log before exit.
- In Log view, returning to Live must not be confused with quitting the app.
- The header should make the active activity visible without adding noisy explanatory text.
- Open Files is an explicit per-process investigation action. It lists disk files currently open by the selected live process.
- Open Files is not a general handle explorer for pipes, sockets, registry keys, events, mutexes, or every possible Windows handle type.
- Open-file collection must not block the UI thread. Refreshing the list should be explicit and should not queue redundant refresh work for the same modal session.
- Process Info tabs must keep the `ProcessIdentity` fixed when the dialog opens. Image, Files, DLL, and Environment worker results must also match the current dialog generation so stale results cannot update a reopened dialog.
- DLL enumeration and file metadata collection must stay on its independent worker and occur only on initial tab activation or explicit refresh, never in normal sampling.
- Environment remote-memory reads must stay on their independent worker, enforce the 4 MiB limit, never enter recording/export data, and never expose values through status or error text.
- Log view must not start live Process Info Image, Files, DLL, or Environment collection. Dynamic tabs display their not-recorded state inside the shared Process Info dialog.
- Loading a named Tracked List may prune older retained history for names removed from the working list. Confirm before discarding those older samples.

## UI / UX Guide

- Keep the TUI compact and low-noise. Do not add unnecessary borders, spacing, explanatory text, or decoration.
- Keep clipboard output raw and minimal so it can be pasted as-is. Do not add unnecessary headers or explanations.
- Buttons such as OK / Cancel at the bottom of dialogs must be operable by mouse click as well as keyboard.
- Format shortcut guidance inside confirmation dialogs exactly like the screen footer: show each key first in the muted style, its action label in the normal text style, and separate bindings with two spaces. Do not use prose-like slash-separated copy such as `Enter selects / Esc cancels`.
- Detailed user-facing controls and UI behavior belong in README, Help, Footer, tests, and implementation. Metric definitions belong in `docs/metrics.md`; design-level boundaries and invariants belong in `docs/architecture.md`.
- When changing controls or UI behavior, update the canonical user-facing documentation and tests together. Help, Footer, README, tests, and actual key handling must stay aligned.

## Implementation Review Points

- Compute drawing regions and mouse hit-test regions from the same helpers and conditions. If Graph / Samples visibility, Delta visibility, or multiple slots make drawing and input regions diverge, clicks and cursor lines will break.
- When Graph and Samples operate on the same concept, check for missing key-operation parity. For example, sample cursor movement should have matching meanings for Home / End / PageUp / PageDown / Left / Right in both Samples and Graph.
- Do not confuse "nearby sample" with "sample that actually exists at that time." Cursor movement and mouse selection may choose a nearby sample, but Graph should show a value only when that Graph has a sample at the same captured time.
- For multiple Graphs, separate shared state from slot-specific state clearly. Time span, A/B points, and cursor age may be shared, but Y-axis scale, sample availability, and value labels must be checked independently per Graph.

## Testing and Verification

- After Rust changes, run `cargo test` whenever practical.
- Do not require every test to run merely because a branch was pushed. Choose verification based on the risk and scope of the change.
- If normal build or test commands fail because the executable is locked, consider using a separate target directory such as `CARGO_TARGET_DIR=target/codex-build`.
- For UI changes, consider whether existing `TestBackend` drawing tests or buffer snapshot tests can cover the behavior.
- When a specification changes, also check whether `README.ja.md` and `README.md` need updates.

## Commands

Use PowerShell / Windows-oriented commands.

```powershell
cargo test
cargo build
cargo run --release
```

Use focused tests or `cargo test <name>` when appropriate.
