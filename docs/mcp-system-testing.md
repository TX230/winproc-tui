# MCP system testing

This procedure tests the implemented tools in the [MCP integration specification](mcp-integration-specification.md). All 26 tools are advertised. Passing the listed checks is not a claim that every environment-specific conformance scenario has been exercised.

## Reproduction

Use Windows 11 x64, Python 3.12 or later, and a debug build from `cargo build`. Start `target/debug/winproc-tui.exe` interactively and obtain its PID. Run clients in the same logon session and integrity level. Keep generated evidence under ignored `notes/`.

```powershell
python -B scripts/test-mcp-system.py --exe target/debug/winproc-tui.exe --pid <tui-pid> --output notes/mcp-system.json --soak-seconds 300
python -B scripts/test-mcp-pipe.py notes/mcp-pipe.json
```

The system harness creates and terminates only its own bridge and uniquely named helper processes. The helper executable is copied beside the output file and removed after use. A unique executable name avoids interference from ordinary two-generation retention of other same-name processes. These scripts do not modify Tracking Lists, profiles, or Recording state. Process and file metadata in result files may be sensitive; keep evidence local.

For independent clients, install the Python `mcp` package in a local virtual environment and the TypeScript `@modelcontextprotocol/sdk` package in a local npm directory. These are test dependencies, not application runtime dependencies.

```powershell
<venv-python> -B scripts/test-mcp-python-sdk.py target/debug/winproc-tui.exe <tui-pid> notes/mcp-python.json
node scripts/test-mcp-typescript-sdk.mjs target/debug/winproc-tui.exe <tui-pid> <sdk-package-directory> notes/mcp-typescript.json
python -B scripts/test-mcp-settings.py target/debug/winproc-tui.exe <tui-pid> notes/mcp-stage.txt notes/mcp-settings.json
```

The Settings script prints coordination prompts. After READY, select Disable MCP connections in the TUI and write `disabled` to the marker file. After DISABLED VERIFIED, clear that checkbox and write `enabled`. It verifies connection invalidation, reconnection, rejected old references, and continued sampling. Separately verify display pause, then disable MCP, quit normally, restart, and verify connection refusal. Re-enable and quit normally to restore the test configuration.

## Initial read-adapter verification: 2026-10-31

Windows debug-build checks completed:

- Live harness: 17 checks passed, including invalid arguments and recovery, connection isolation, immutable pagination, exact raw/summary agreement, six native capture kinds, eight-capture capacity and release recovery, ended-lifetime discovery from a new connection, rejection of live inspection of an exited process, cancellation, four-client capacity, 20 reconnects, and post-load responsiveness.
- Raw named pipe: seven checks passed, covering zero/oversized frames, invalid UTF-8, malformed JSON, incompatible private protocol version, fragmentation/coalescing, and cancellation races.
- Python MCP SDK 2.3.0 and TypeScript MCP SDK 1.32.2 independently negotiated MCP 2025-11-25, discovered 12 tools, and completed process discovery, metric reads, and open-file capture/read/release with structured/text parity.
- Display pause left MCP sample timestamps advancing. Settings disable closed an existing connection and refused new ones; re-enable required fresh references. Across that test, 46 history samples had a maximum interval of approximately 1.036 seconds.
- A saved disabled setting refused connections after restart. Re-enabling restored the listener; the test configuration was restored to enabled on normal exit.
- Baseline 300-second and corrected 180-second sampling runs completed. Concurrent investigation and client tests ran during these intervals, so they are not idle benchmarks or a controlled enabled/disabled comparison. They do not establish absence of long-term leaks or release-build performance.
- Rust tests: 1,149 passed, ten ignored; four CLI integration tests passed. Formatting and Clippy with warnings denied passed.

System testing exposed and corrected frozen-page timestamps being replaced by newer snapshot timestamps, partial capture status/coverage being lost on page reads, and unnecessary extra capture allocation when reading an existing unfiltered capture at capacity. Regression tests cover metadata, expiry inheritance, and reading at the full capture budget.

## Complete tool workflow verification: 2026-10-31

Use an isolated executable/configuration and an isolated configured Recording directory for mutation tests. The workflow intentionally changes the Current Investigation, creates and deletes test profiles, and creates recording logs. Its confirmation prompts identify the exact test action; an operator confirms it in the TUI. Pass a separately built, readiness-signalling .NET helper to exercise collection without attaching to user applications.

```powershell
python -B scripts/test-mcp-workflows.py <exe> <tui-pid> notes/mcp-workflows.json <ready-helper-exe>
python -B scripts/test-mcp-log-view.py <exe> notes/mcp-log-view.json
python -B scripts/test-mcp-load.py <exe> notes/mcp-load-stage.txt notes/mcp-load.json
```

The optional .NET helper used in this verification was a .NET 10 console app that printed `READY` after initialization, then repeatedly allocated 1 MiB and slept 100 ms. The harness owns and terminates that helper. The log-view script prompts for Ctrl+L/Enter and Ctrl+B. The load script prompts for the local Settings change and marker values `disabled` and `enabled`.

Both debug and optimized builds completed process metadata, tracking by reference/name, stale-revision rejection, profile save/save_as/rename/load/delete, process appearance and exact metric threshold waits, Recording start and confirmed stop, source-isolated saved-log metrics/history, explicit .NET collection through Collecting and revocation, and held-handle process-exit detection. Recording tests used an isolated locally configured directory and generated filenames.

Python MCP SDK 2.3.0 and TypeScript MCP SDK 1.32.2 negotiated 2025-11-25, discovered all 26 tools, and completed process/metrics/file investigation on the optimized build. Their strict tool-list validation caught a missing top-level output-schema `type: object`; the catalog and a regression assertion now include it. These SDK tests were rerun successfully after the correction.

The optimized live harness passed all 17 read/lifecycle checks and the raw-pipe harness passed all eight framing/cancellation/backpressure checks. Entering TUI Log view rejected live queries, invalidated existing live captures, and cancelled a live wait; an independent loaded log remained queryable both in Log view and after returning to Live. A real-pipe Rust test, run separately with the TUI stopped, passed four simultaneous client connections, round-trip delivery, and endpoint-squatting rejection.

The complete Rust suite passed 1,160 tests with ten ignored, plus four CLI integration tests. The native endpoint test above was then run explicitly rather than counted as an implicit pass. Added regression coverage includes confirmation timeout/disable, stale confirmation revision, 120-sample pruning, operation deduplication and capacity, Recording ownership, interrupted versus malformed logs, log budget/cancellation, source-reference expiry, integer thresholds, and automatic response page reduction. Formatting and Clippy with warnings denied passed.

The stalled-reader test exposed a Windows handle-lifetime issue: cancelling tasks and closing handles alone could leave a synchronous peer blocked in WriteFile. Session teardown now explicitly disconnects the pipe. The test runs the flooding client in an isolated child so a failed check cannot hang cleanup. A concurrent connection check also exposed transient pipe-busy opens; bridge connection now retries those within its existing two-second deadline.

A controlled optimized-build comparison used 15 seconds each with MCP idle, three request clients plus the observation connection, and MCP disabled. All three phases retained 15 system samples. The three clients completed 78,431, 79,051, and 78,786 metric requests without errors; maximum sample gaps were 1.030078, 1.082683, and 1.029038 seconds respectively for idle, load, and disabled phases. These short local measurements verify continued one-second sampling, not a general throughput guarantee. Run this test without other clients, because it deliberately uses all four connection slots.

Packaging verification ran `scripts/package-release.ps1 -SkipTests -SkipBuild` after the separately completed test/build checks. All-features Clippy, `cargo package --locked` and its source-package build, static Microsoft CRT import checks, runtime-only ZIP entry validation, and checksum generation passed. The extracted executable matched the tested release binary by SHA-256 and its stdio bridge advertised all 26 tools without a running backend. These are local candidate artifacts using the repository version, not a published release. Test TUI/helper processes were stopped and the isolated configuration retained MCP enabled.

## Remaining environment coverage

Cross-user logon and a genuinely elevated or lower-integrity client have not been exercised end to end; token comparison rejection has unit coverage. Remote-host rejection, UNC failure modes, forced OS PID reuse, and multi-hour leak behavior remain environment-specific release checks. Signature verification is explicitly unavailable in process metadata. Ignored tests are not counted as passes, and short local load checks do not establish behavior under every file provider or workstation workload.

## Adversarial review follow-up: 2026-10-31

The implementation at `d21b263`/`1557f86` was reviewed again with mixed-source capacity and operation-lifecycle system checks. Earlier successful workflow checks are not sufficient for release approval. Run the added harness only against an isolated executable/config with at least one completed `winproc-mcp-*.log` in its configured adjacent `logs` directory:

```powershell
python -B scripts/test-mcp-adversarial.py <isolated-exe> notes/mcp-review.json
```

The harness writes uniquely named log fixtures and deletes them afterward. It creates a uniquely named profile and deliberately leaves it saved: both rejected deletion attempts must preserve it. No local confirmation should be accepted during this test. The confirmation expiry check takes approximately 60 seconds. `--quick` skips that operation-lifecycle section. The original reviewed build returned nonzero; the corrected build is expected to pass all scenarios.

Additional successful checks: incomplete final JSON preserves earlier frames and reports partial; malformed complete JSON fails; a line over 4 MiB is rejected; completed operations survive bridge reconnection and identical resubmission; disconnect cancels pending approval without deleting its profile.

Three failures were reproduced on the optimized executable:

- A loaded log plus seven child captures can coexist with a subsequently completed native capture: nine retained objects despite the advertised combined limit of eight. Completion accounting omits loaded-log child captures. The check must cover allocation at both request acceptance and completion, including child-derived pages and total retained bytes.
- Eight frozen process-list captures prevent an unmodified `read_capture` of an existing capture. Only native captures with an initially empty signature take the reuse path; list/history captures attempt a new allocation. Existing coverage tested the native-capture case only.
- A pending profile deletion expires with `CANCELLED`, although the error contract distinguishes timeout from cancellation and direct confirmation handling already uses `TIMEOUT`. The profile remains intact; the defect is the observable reason code.

Static review also found double-counting in `query_log`: the pre-query connection total already includes the log child captures, but the post-query check adds their entire total again rather than their increase. This can reject valid queries below 64 MiB; a near-budget end-to-end fixture has not yet been exercised.

These failures were recorded under Issue #202; the review commit added executable regression scenarios without changing production behavior. See the correction verification below for their resolution. Cross-logon/integrity, forced PID reuse, UNC failure, five-minute expiry and multi-hour leak scenarios are still not covered end to end. Confirmation acceptance key/mouse behavior, filesystem write/flush failure injection, and concurrent capture/log memory reservation also need dedicated coverage before release approval.

## Review correction verification: 2026-10-31

All four review findings are corrected. Completion accounting now includes saved-log child captures and pending log slots. Derived child pages are checked against connection-wide count and byte budgets and rejected without removing their source. Unmodified reads reuse the frozen capture and preserve its creation time and original query cursor. Log query accounting replaces the previous child-byte contribution with the new contribution. Confirmation expiry reports `TIMEOUT`, while disconnect/disable/refusal remain cancellation.

The optimized build passed every adversarial harness scenario, including the real 60-second confirmation expiry. The mixed-source scenario retained eight objects, and the full-capacity read succeeded. The existing system harness also passed all 17 checks (`--soak-seconds 0`, so this rerun adds no long-duration evidence).

Three new Rust tests verify source-cursor preservation at full capacity, late job completion and derived-child allocation against the global count, and near-64-MiB accounting with rollback preserving original captures. The memory-boundary test uses a synthetic loaded-log reservation plus an actual 1-MiB child capture; it demonstrates that a valid small addition succeeds and an oversized projection fails without double-counting. It is not a near-limit end-to-end log-file benchmark. The existing confirmation regression now asserts the exact timeout code. Full validation passed 1,163 Rust tests (ten ignored) and four CLI tests, formatting, and all-target/all-feature Clippy with warnings denied.

The pre-existing environment-specific coverage limits above remain unchanged. README content is unaffected by these corrections to the specified contract.

## Operational use-case verification: 2026-10-31

Rechecked the specification against the command dispatcher, private handshake, published control state, tool catalog, and the corrected optimized executable. Removed obsolete proposal-only wording for the implemented bridge/endpoint. Corrected the response example (`samples[].target`), profile-list inputs, and the location of active-profile information and capability/limit discovery. Added the operationally important `OUTCOME_UNKNOWN`, `RESOURCE_LIMIT`, and `COLLECTION_FAILED` error codes to the documented list. The specification now explicitly distinguishes currently returned fields from absent Recording elapsed/latest-outcome and .NET runtime-classification/freshness fields; these absent fields were documentation discrepancies, not newly implemented features. Operation outcomes remain available through `get_operation`.

The operational run used an isolated configuration, a configured local log directory, the optimized executable containing the review corrections, and a separately owned readiness-signalling .NET 10 helper. Only confirmations naming the test operations were accepted. No user workload was attached or terminated.

| Use case | Observed result |
|---|---|
| Find a process, inspect metadata, add tracking by reference and name | Passed; stale revisions rejected. |
| Save, rename, modify, reload and delete an Investigation Profile | Passed with local confirmation for replacement/deletion. |
| Wait for appearance, a metric threshold, and verified process exit | Passed. |
| Start Recording, disconnect the bridge, reconnect and stop | Same session and fixed tracking scope survived reconnection; confirmed stop succeeded. |
| Start a replacement recording and send the old stop ID | Old stop failed with `STATE_CONFLICT`; replacement remained active. |
| Stop a 10-second aggregated recording mid-window | The saved log loaded as complete; its final two frame timestamps were 4,060 ms apart and a clean end record was present. |
| Inspect saved process metrics/history and try the log reference against Live | Saved data read successfully; live misuse was rejected; explicit closure invalidated the log reference. |
| Authorize initialized .NET helper, observe collection, revoke permission | Reached `collecting`; revocation and held-handle exit detection succeeded. |
| Disable/re-enable MCP locally while Recording | Existing connection closed, new connection refused while disabled, old references rejected after re-enable, and the original recording session/scope survived. 51 Live samples spanned the check with a maximum gap of 1.029118 seconds. |
| Enter TUI Log view while an independent MCP log and live wait exist | Live requests rejected, live capture invalidated, wait failed, and independent saved-log analysis remained available both in Log view and after returning to Live. |

Reproduction additions:

```powershell
python -B scripts/test-mcp-workflows.py <isolated-exe> <tui-pid> notes/mcp-operations-workflows.json <ready-helper-exe>
python -B scripts/test-mcp-settings.py <isolated-exe> <tui-pid> notes/mcp-operations-stage.txt notes/mcp-operations-settings.json --recording
python -B scripts/test-mcp-log-view.py <isolated-exe> notes/mcp-operations-log-view.json
```

The workflow harness now includes bridge reconnection during Recording and stale-stop protection for a replacement session. The Settings harness optionally starts a recording, verifies ownership across disable/re-enable, and requests a locally confirmed stop. The workflow reported 11 successful groups; both Settings and Log-view harnesses passed. Catalog/schema tests passed (two tests). Production Rust code was unchanged in this documentation/test follow-up. All test Recording sessions and helper processes were stopped, and the isolated TUI was closed normally with MCP enabled.

These scenarios are operational evidence, not complete conformance certification. Cross-logon/integrity, forced PID reuse, UNC failure, injected disk-write/flush failure, five-minute expiry, and multi-hour/24-hour behavior still require dedicated runs; previously documented unit tests do not substitute for those environments.


## Visible application startup verification: 2026-11-01

The bridge now starts an absent application in a new Windows Terminal window on the first validated tool call. The startup contract and limitations are owned by the [MCP specification](mcp-integration-specification.md#3-startup-and-connection-lifecycle) and [ADR 0004](adr/0004-visible-mcp-application-startup.md).

The new harness requires an isolated executable directory with no configuration and no existing TUI. It creates private launcher fixtures beneath that directory, changes only child-process environments, and deliberately leaves the real application running after successful launch:

```powershell
python -B scripts/test-mcp-startup.py <isolated-exe> notes/mcp-visible-startup.json
python -B scripts/test-mcp-startup.py <isolated-exe> notes/mcp-visible-restart.json --phase restart --pid <owned-tui-pid>
# After normally closing the prior test TUI; symlink creation must be permitted:
python -B scripts/test-mcp-startup.py <isolated-exe> notes/mcp-visible-alias.json --phase alias
```

Fourteen startup checks passed on the optimized executable:

- Initialization, discovery, ping and invalid calls do not start an application or write configuration.
- A saved disabled preference and malformed configuration prevent automatic startup without rewriting the configuration.
- An absent Terminal alias and a relative application-data location produce actionable errors without fallback.
- A fixture launcher exiting nonzero is reported; its stdout/stderr cannot corrupt MCP output.
- A fixture launcher exiting successfully without a backend times out after approximately 12 seconds and is invoked only once.
- Existing application and launch-coordinator markers prevent replacement startup. These checks deliberately model an unavailable listener; they do not claim to distinguish runtime disablement from listener failure.
- Cancellation while waiting for another coordinator prevents launch after that marker is released.
- Cancellation after a fixture launch retains coordination until the startup deadline and does not replay the tool.
- EOF during an unfinished startup exits the bridge normally and releases the marker. The test client allows eight seconds because the MCP SDK can drain pending responses for five seconds; the earlier five-second test-client kill deadline raced that SDK drain.
- Two actual bridges starting concurrently connect to the same backend instance. The executable path includes spaces, Japanese text, an ampersand and a semicolon.
- Four clients reuse that backend; a fifth receives an error without a replacement launch.
- Closing every bridge leaves the application running, and a new bridge reconnects to the same backend instance.

Two additional checks used a real executable symlink: a disabled legacy configuration blocked startup without migration; after enabling it, ordinary TUI startup migrated the launcher-adjacent file and restored its Tracking List. Review corrected the launcher to preserve the verified absolute launch path instead of jumping directly to the canonical target, which would bypass that migration. The alias fixture initially used an incorrect nested Tracking List field; it was corrected to the actual flattened configuration schema before rerunning successfully.

The restart scenario passed three further checks: send a normal console termination signal to the exact test image, report the old connection as OUTCOME_UNKNOWN without replay, and launch/reconnect on the next explicit request with a different backend instance ID. The shutdown helper verifies the process image before signalling its console; it does not synthesize terminal commands or input. Normal shutdown persisted the isolated configuration. All TUI processes started for this verification were subsequently closed through normal console-control cleanup, so the test did not leave the application single-instance marker occupied.

The existing 17-case read/lifecycle system harness passed against the automatically started application, including metrics/history, module/file/thread/network captures, capacity, retained lifetimes and 20 reconnect cycles. No additional soak duration is claimed. Rust validation passed 1,168 tests with ten ignored and four CLI tests, plus formatting and all-target/all-feature Clippy with warnings denied. Five added Rust cases cover launch-error classification, literal Terminal arguments, saved/legacy preference precedence without writes, early cancellation and non-owning single-instance probes.

Process inspection confirmed a single test TUI parented by Windows Terminal in the interactive Windows session, and Terminal had a desktop window handle. Screenshot-based visual verification remains manual: the desktop automation inventory did not expose Windows Terminal in this environment. The optional harness phases `disabled` and `reconnect` support a human Settings toggle, but were not executed in this run. Earlier Settings lifecycle evidence is recorded above. Startup profile-selection and custom/elevating Terminal profiles also remain manual checks; no automatic profile choice or elevation is implemented. Previously listed cross-logon/integrity, UNC, disk-failure and long-duration conformance limits remain unchanged.


## Interactive color correction: 2026-11-01

The maintainer confirmed that the automatically started TUI was visible when it was kept open for inspection, but its text was monochrome. The configured theme was Green. The MCP client's environment included `NO_COLOR=1`, `TERM=dumb` and an empty `COLORTERM`; inheriting that environment caused crossterm to suppress ANSI colors.

The launcher now explicitly uses Windows Terminal's `--reloadEnvironment` instead of `--inheritEnvironment`. This restores the normal user environment without globally forcing colors, editing Terminal settings or overriding a NO_COLOR preference stored in the Windows user environment. The command-construction regression checks the selected mode.

A real Windows Terminal probe compared both modes with injected client-only environment values and a test marker:

```powershell
python -B scripts/test-mcp-terminal-environment.py notes/mcp-terminal-environment.json
```

Inheritance reproduced all four values. Reloading removed all four in this environment, including color suppression; the probe also checked the normal user/machine NO_COLOR preference. It records only boolean checks, not arbitrary environment contents. The probe does not alter global settings and its helper exits after writing the result. Full Rust validation passed 1,168 tests (ten ignored) and four CLI tests. After restarting the corrected TUI through the actual MCP bridge, the maintainer confirmed both the visible window and colored rendering. The application was left open after bridge EOF for continued human use. Process/window handles alone are not accepted as visual confirmation.


## Full specification audit (2026-11-01)

Reviewed all sections of the MCP specification against the committed tool catalog, bridge/startup/transport, query and job adapters, and application-owned actions. Corrected proposal-era wording and overstated contracts for history coverage, open-file fields, schema completeness, pagination, error codes, cancellation, condition semantics and listener-versus-application lifetime. The tool catalog descriptions now match returned status/host fields, job polling, tracking provenance, log discovery and condition waits.

The audit reported two suspected provenance problems. Follow-up implementation review below confirmed the saved-log generic-read defect but disproved the general-getter claim: the audit had overlooked separate `log_view_display` storage. Runtime behavior was not changed in that documentation-only audit. The inaccurate Log-view limitation has been removed from the specification.

Validation: `cargo test mcp::` passed 23 tests with one native endpoint test intentionally ignored; the focused stdio CLI test passed initialization, discovery, invalid-argument handling, JSON result parity and EOF shutdown. The endpoint test requires the TUI to be stopped and was not run. Catalog comparison confirmed that only six descriptions changed, with all 26 tool names, annotations and input/output schemas preserved. Documentation links, UTF-8/LF encoding and `git diff --check` were checked. No new end-to-end UI or collection scenario was run; earlier evidence and remaining release checks above retain their stated scope.


## Saved-log provenance correction (2026-11-01)

A new regression first reproduced the generic-read defect against the previous implementation: the direct process list returned recorded lifecycle semantics and a log reference, while reading its frozen capture returned a live lifecycle and omitted the log reference. Interrupted-session status/warnings were also absent from the generic path.

Process rows now receive recorded lifecycle semantics before capture storage and projection. Direct saved-log queries and generic capture reads share envelope assembly, preserving source, log reference, interval, timestamps and partial status while merging warnings without duplicates. Frozen capture source metadata is retained by the common envelope.

Regression coverage includes complete/interrupted logs, direct and generic pages, filtered/projected captures, rereading derived captures, raw history, source/capture warnings, error passthrough, original expiry preservation and closure invalidation. A separate application test loads deliberately different recorded CPU/memory/process/time values and verifies that general MCP host/tracking data still uses the last Live snapshot through Log view and return to Live. This disproved the earlier mixed-source diagnosis; no production ownership change was necessary for that case.

Full validation passed 1,171 Rust tests (ten ignored) and four CLI tests. All-target/all-feature Clippy with warnings denied passed. These are application/adapter regression and CLI checks; no new visible-TUI end-to-end session was run, and the previously documented environment-specific release checks remain outstanding.

## Registered Codex client verification (2026-11-01)

Registered the built release executable as the user-level Codex MCP server named `winproc`, using stdio and the `mcp-bridge` argument. `codex mcp get winproc --json` confirmed the executable command, enabled state and argument. The tested executable SHA-256 was `E81350B0F04E9CB476F60465CC04A39FA54D353007812E4C5C29A44E4DEDF64A`.

A fresh ephemeral Codex CLI 0.164.0 session loaded that registration and performed the investigation through native `mcp_tool_call` events. It called `winproc_get_status` twice, `winproc_start_capture` once (`file_users`), `winproc_get_job` once, `winproc_read_capture` once (`filter: ".log"`, `limit: 500`) and `winproc_release_capture` twice. All seven calls completed without MCP errors. No Python, custom protocol client or shell command was used inside the test session. This tests registered Codex client integration, not only the earlier protocol harness. The already-running desktop chat did not expose the newly registered tools, so the native calls were made by the fresh CLI session.

Capture ran from `2026-11-01T08:54:38.893673500Z` to `2026-11-01T08:54:39.536525600Z` (about 0.643 seconds). The filtered capture returned 185 rows with no continuation cursor. Exact case-insensitive owner-name and extension checks selected 16 rows for `chrome.exe` ending in `.log`. Every selected row had one handle. The job succeeded with partial coverage: 255 of 405 processes inspected, 150 access-denied processes, 28,912 unreadable handles and three unnamed disk handles. These coverage counts describe the whole capture and do not establish exhaustive Chrome coverage.

Both the source and derived captures were released successfully. The final status reported Live, connected and not stale; the TUI remained open after client exit. Registration remains installed. Local-only evidence: `notes/codex-native-winproc-events.jsonl`, `notes/codex-native-winproc-result.md` and `notes/codex-native-winproc-stderr.log`. A pre-existing unsupported computer-use setting produced a CLI warning but did not prevent MCP calls; that unrelated setting was not changed.

## Active-connection header indicator (2026-11-01)

The activity header now shows the MCP badge only after peer verification and the internal handshake complete. A unique real named pipe regression verifies that a raw connection and an incompatible Hello do not emit a connected event, while a valid handshake does. It does not use or replace the running application's endpoint.

Connection-state regressions cover two clients, a rejected third handshake, partial and final disconnects, and listener failure. TestBackend rendering covers the yellow badge before Live and Log view in all four themes at 50, 80 and 120 columns, unchanged header hit-test geometry, Recording with stale/paused status, and local disablement. These are native transport and application/rendering regressions; no new desktop screenshot or full interactive MCP session is claimed.

Validation passed 1,175 Rust tests (ten ignored) and four CLI tests, formatting, and all-target/all-feature Clippy with warnings denied. README and footer controls are unchanged because this is a status indicator; Help and the owning MCP specification describe it. The change does not introduce a new architectural decision.

## Shared output-schema source (2026-11-02)

The source catalog now stores the common success/error output envelope once and expands it for each tool at runtime. Compared actual `initialize` / `tools/list` responses from the previously validated badge release and the refactored executable: all 26 tool definitions matched as JSON values, including names, descriptions, annotations, input schemas and output schemas. This discovery comparison did not dispatch investigation tools or start another TUI. Local evidence is retained in `notes/discovery-before.json`, `notes/discovery-after.json` and `notes/discovery-parity.json`.

Added regressions cover source-field preservation, absence of source-only fields in discovery, isolated per-tool success-data overrides, unchanged common error validation and rejection of duplicated source output envelopes. The stdio CLI test checks expanded output schemas and validates a real error response against the advertised contract. Full validation passed 1,180 Rust tests (ten ignored) and four CLI tests, formatting, and all-target/all-feature Clippy with warnings denied.
