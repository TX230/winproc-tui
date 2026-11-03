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

## MCP performance and bounded completion (2026-11-03, Issue #205)

The performance investigation covers all interface categories, with file capture as a model-driven reproduction. Direct official TypeScript SDK measurements of the unchanged baseline showed approximately 0.285 ms median status latency and 33–37 ms to fetch eight capture pages. Collection plus retrieval varied from 1.71 to 4.94 seconds. The dominant delay in the native Codex workflow was between client operations; collector time alone did not explain its 55.74-second path retrieval.

Opt-in bridge completion now applies to investigation, metadata, log-list/load and condition jobs, plus application-operation outcome waits. Capture jobs can include the first selected page. All paged interfaces accept up to 5,000 rows under the existing response-size guard. File-owner/extension selection and distinct path projection reduce unrelated rows without changing collection coverage. Shared live-page reads avoid unnecessary retained-log byte-accounting serialization, and status reads no longer clone the complete tool schemas.

Native Codex runs with the same configured model measured path retrieval at 55.74 → 14.57 seconds with high reasoning and 20.76 → 7.75 seconds with low reasoning. Initial-request-to-result latency changed from 30.78 → 0.78 seconds and 11.61 → 0.74 seconds respectively. Calls including cleanup fell from 14/16 to three. These are observed workflow comparisons with revised tool guidance and prompts, not controlled model-service benchmarks: host state, collection duration and client startup varied. No collector algorithm was changed, and these observations do not establish a native scan speedup. Client reasoning settings were overridden only for the measurements; no global model/reasoning preference was changed.

Three optimized direct captures completed in 0.69–1.02 seconds, each returning the distinct path page in one call. Exact same-capture comparison verified every path, Chrome .log rows and empty partial results against the unfiltered source. Cross-category checks covered cached getters, metrics and raw/summary history, Network, Threads, Scheduling, Open Files, modules, process metadata, saved-log listing/loading/query/closure, conditions and unknown operation lookup. During a 1.5-second condition wait another connection's status request completed in about 4.2 ms. Application mutations and required confirmations are covered by existing application regressions and new bridge tests; no destructive or user-state-changing benchmark was performed.

Run the read-only category benchmark with an installed official TypeScript MCP SDK package and a running test TUI. The optional mode is async or completion (default). A configured Recording root is required for the saved-log subcheck; otherwise its unavailability is reported. The benchmark prints per-category evidence and writes request duration/result-byte measurements to the specified output:

    node scripts/benchmark-mcp.mjs <exe> <tui-pid> <sdk-package-directory> <output.json> completion

Validation passed 1,196 Rust tests (11 intentionally ignored), four CLI tests, formatting and all-target/all-feature Clippy with warnings denied. Local native events, monotonic timings and direct-client evidence are retained under notes/mcp-perf-20261103/. Model/client startup and service latency remain outside the server's control; the specification and ADR 0006 describe the bounded interface improvement without promising an end-to-end latency ceiling.

## First-call generation follow-up (2026-11-03, Issue #205)

Added an unmodified stdio timing proxy to measure the previously missing tools/list-to-tools/call boundary. Discovery itself took about 1.7 ms in the first traced run and sent 26 tools in approximately 51.8 KB. Three native Codex/Astra-high status workflows spent 5.70–6.97 seconds after discovery (median 6.35 seconds). Explicitly requesting the tool as the first action, without a separate preamble, reduced that interval to 4.06–4.88 seconds (median 4.83 seconds, three correct runs). These are repeated observations, not a latency guarantee.

A diagnostic OpenTelemetry run of the direct-prompt case placed a 4.08-second model API stream wait inside a 5.85-second post-discovery interval. Tool-router construction was about 32 ms and world-state construction about 281 ms in that run; other initialization/prewarming accounted for additional time. A normal-prompt trace with an extra developer directive still made two main-model sampling requests before the tool: approximately 3.14 and 2.81 seconds. A tool schema cannot eliminate a client model's inference/service wait.

Negative controls did not support reducing the public contract: advertising only the status tool still took 7.98 seconds after discovery in one run; removing optional output schemas took 4.24 seconds, within the observed direct-prompt spread. A smaller-context Astra-low run took 3.96 seconds. Luna-low with a smaller context and explicit direct prompt took 3.06–3.37 seconds across three correct runs, but this combines model/context changes and is not an isolated model comparison. No global model or service-tier setting was changed. An aggressive dedicated profile selected the wrong tool in two of three runs (invalid empty-argument cancellation calls were rejected); it was not adopted. Correctness failures are excluded from successful latency statistics.

Added the direct workflow runner to avoid spawning a second Codex/model session for already-known MCP request sequences. In the maintained-script check, connect plus discovery took about 71.8 ms, discovery to first call about 0.09 ms, and the status workflow completed in about 76.7 ms. The full distinct-path workflow took about 1.41 seconds including collection, complete pagination and cleanup, with about 0.15 ms from discovery to its first call. These intervals measure execution of caller-authored workflows; they exclude the current assistant's prior reasoning and are not comparable to natural-language-to-tool inference latency. Standard native MCP remains available with its measured model/client delay.

Reproduce native timing (read-only status request; existing model preserved):

    ./scripts/benchmark-mcp-first-call.ps1 -Executable <exe> -OutputDirectory <evidence-directory> -Direct

Omit -Direct for the normal-prompt comparison. The script rejects a wrong first tool or failed status response and writes a monotonic discovery-to-call interval. Run a known workflow from the current client without a nested AI session:

    node scripts/run-mcp-workflow.mjs <exe> <sdk-package-directory> scripts/mcp-workflows/status.mjs <output.json>
    node scripts/run-mcp-workflow.mjs <exe> <sdk-package-directory> scripts/mcp-workflows/file-paths.mjs <output.json>

Both maintained examples and the passive native timing harness ran successfully. No Rust production code, catalog schema or application setting changed in this follow-up; Rust verification from the preceding implementation remains applicable. Diagnostic catalog rewrites, model comparisons and rejected profiles remain local experiments. Their traces and the raw timing evidence are retained under notes/mcp-first-call-20261103/.

## Native same-session latency and tool exposure (2026-11-03, Issue #205)

Further native Codex CLI 0.164.0 app-server measurements used Astra/high with all 26 tools, one ephemeral conversation per session, and four fresh status requests per session. Each session initialized/discovered the bridge once and reused it for subsequent turns. The boundary below is **turn/start submission to first real MCP tools/call**, including model/client work; it differs from the preceding discovery-flush boundary and excludes execution of the MCP request itself. There were three sessions per group, giving three first-turn and nine subsequent-turn observations:

| Prompt and exposure | First turn, median (range) | Subsequent turns, median (range) | Model requests before first-turn MCP call |
| --- | --- | --- | --- |
| Normal request, full surfaces | 9.36 s (6.62–10.87) | 4.75 s (2.81–10.73) | 2 in each session |
| Explicit tool name, `{}` and no preamble, full surfaces | 5.04 s (4.34–5.60) | 3.37 s (3.09–8.10) | 1 in each session |
| Same normal request, direct-only exposure | 6.95 s (5.00–7.43) | 3.98 s (3.14–7.63) | 1 in each session |

For the exact discovery boundary requested in this investigation, monotonic tools/list-flush-to-first-tools/call medians across the same first turns were 9.271 s (6.533–10.777) for normal/full, 4.957 s (4.257–5.526) for the explicit prompt/full, and 6.862 s (4.918–7.337) for normal/direct-only. Discovery flushed about 79–95 ms after turn submission in these runs. Subsequent turns did not repeat discovery.

A diagnostic baseline classified the internal Code Mode calls without retaining their arguments: the first inspected `ALL_TOOLS`, then the second invoked the status tool. With `omit_tools_from = ["deferred", "code_mode"]`, normal requests still emitted preambles but called the native MCP tool with one model request. This refines the preceding preamble explanation: catalog inspection is a demonstrated extra step; the explicit prompt changes tool naming, argument guidance and preamble together, so its improvement cannot be attributed to prose suppression alone. All 36 status requests in the three main comparison groups were correct. The direct-only setting retains all 26 direct tools and their validation/authorization rules.

The direct-only normal-prompt medians were about 26% lower for first turns and 16% lower for subsequent turns. These small repeated samples have broad, overlapping ranges and are not randomized service benchmarks or guarantees. Subsequent normal/full turns already used one model request: their median model time-to-first-token was 3.862 s, followed by about 0.773 s of response streaming. Direct-only subsequent medians were 3.548 s and 0.438 s respectively. One normal/full subsequent request reused 38,656 of 38,862 input tokens (99.5%) yet still reported 3.915 s to first token. Warm connections and a hot prompt cache therefore do not eliminate this delay. Time-to-first-token includes client/network/provider scheduling, prefill and model waiting; the available telemetry cannot isolate these components or establish a performance floor.

A user-authorized Fast comparison used a separate direct-only session with the explicit direct prompt. After a standard warm-up, three standard turns alternated with three Fast turns. Standard first-call times were 4.684, 3.542 and 2.265 s (median 3.542); Fast times were 2.595, 2.908 and 4.397 s (median 2.908). Telemetry confirmed `service_tier: priority` on the Fast requests. Total turn medians, including the final answer, were almost equal: 6.010 versus 6.006 s. This does not establish a reliable end-to-end Fast benefit. Fast consumes additional usage under the client's pricing rules; the global tier/model/reasoning preferences were left unchanged.

Correctness controls found an invalid `metrics` argument instead of `metric_ids` in both full and direct-only routes. A separate over-constrained prompt also attempted an invalid empty cancellation call; it was rejected and excluded from successful timing statistics. Tool descriptions now prefix the names already required by their input schemas. Three native direct-only workflows with this exact description change each completed host information, live system CPU metrics and full accessible distinct-path retrieval without a tool error. They returned 2,279, 2,339 and 2,316 unique paths respectively; each matched its capture's listed count, exhausted its cursor and released both source and derived captures. All three captures were explicitly partial, so these counts do not imply access to every OS handle. These small controls support the hint but do not prove elimination of all model argument errors. No application mutation or confirmation behavior was changed or benchmarked anew.

Reproduce the native session comparison with Node.js, an installed Codex CLI JavaScript entry point, a test executable and separate empty output directories:

```powershell
node scripts/benchmark-mcp-session.mjs <codex-cli.js> <exe> <output-directory> <model> normal high 4 full default
node scripts/benchmark-mcp-session.mjs <codex-cli.js> <exe> <other-output-directory> <model> normal high 4 direct-only default
```

Use `direct` instead of `normal` to test the explicit prompt. Only use the optional `alternating` tier mode when additional-usage comparisons are intended; seven turns provide one standard warm-up and three turns per tier. The harness explicitly resets exposure in both groups, does not modify global configuration, rejects unexpected approval/input requests, verifies ephemeral conversation ownership and preserves evidence of wrong calls as failures. The local OTLP collector stores only allowlisted timing/token/tool-name metadata, excluding prompts, arguments and results; local app-server events separately contain the benchmark prompts and status responses. The metadata listener is diagnostic infrastructure, not an MCP transport.

The maintained benchmark passed two standard native turns. After installing the rebuilt executable and the winproc-only client setting, a separate ephemeral Codex run using the actual registration (without a proxy or command/argument overrides) made exactly one successful native status call and returned Live/non-stale. Discovery advertised all 26 tools with the new hints; Recording remained inactive. Focused Rust MCP checks passed 36 tests with one intentionally ignored endpoint test, four CLI tests passed, and the release executable built successfully. The source change is limited to 18 descriptions; names, annotations and schemas are unchanged. The preceding full Rust/Clippy verification remains applicable to the unchanged Rust implementation. Raw controlled evidence and sanitized analysis are retained locally under `notes/mcp-warm-session-20261103/`.

## Agent-facing defaults and natural requests (2026-11-03, Issue #205)

The normal bridge now owns bounded completion, default first-page delivery, exact continuations and final-page cleanup across paged/async interface categories. Immediate getters remain immediate; action identity/revision/idempotency/local-confirmation requirements remain explicit. All 26 tools are retained. `--raw` preserves the previous asynchronous, caller-retained lifecycle, and `retain_capture: true` supports repeated frozen-data inspection. Input descriptions include schema-validated minimal examples for all 26 tools, metric inputs enumerate the existing 39 identifiers, omitted metric targets select the system, and omitted query source selects live or an explicitly supplied log reference. See [ADR 0007](adr/0007-mcp-agent-delivery-defaults.md), which supersedes the opt-in-only decision in ADR 0006.

The audit covered immediate status/host/catalog/control getters; current metrics and history; process/capture pages; all six native capture kinds and image metadata; log listing/loading/closure; condition jobs; and all action starters/operation status. Common delivery changed where there are mechanical lifecycle steps. Semantic targets, history bounds, metric selection and mutation safety fields remain explicit. Loaded logs still require explicit close because later queries depend on them. No TUI collection cadence, Windows collector algorithm, Recording format or metric meaning changed.

Native Codex/Astra-high/Fast tests used identical natural requests without tool names, JSON, wait/read/retention parameters or lifecycle instructions. They requested current system CPU percentage, all distinct open file paths, and Chrome-owned `.log` paths. The external harness retained full tool results; final answers contained counts/partial status. One complete baseline and one complete final-variant session observed:

| Request | Before: MCP calls (rejected calls), total turn | After: MCP calls (rejected calls), total turn |
| --- | --- | --- |
| System CPU | 6 (4), 21.81 s | 1 (0), 16.26 s |
| Distinct file paths | 4 (1), 17.51 s | 1 (0), 9.77 s |
| Chrome `.log` paths | 3 (0), 17.44 s | 1 (0), 11.28 s |

**CPU timing correction:** the 16.26-second CPU observation also included an HTTP 429 with about five seconds of retry backoff before its only MCP call. It is an observed end-to-end result, not a clean MCP acquisition benchmark. See the CPU latency decomposition below.

The baseline recovered and returned results despite intermediate rejected calls. Its file workflows explicitly released two captures; final-variant workflows needed no release calls. Before/after file captures each returned 2,302 distinct paths and 16 Chrome log paths, matching their listed counts with no continuation. They were explicitly partial captures, not exhaustive access to all OS handles. Automatic cleanup was reported by `capture_retained: false` and independently tested through subsequent invalid-reference and capacity checks.

A repeat final-variant session completed CPU and distinct paths in one error-free call each (14.25 and 9.53 s; the latter returned 2,306 distinct paths with complete pagination). Its third turn made **no MCP call**: the model API returned HTTP 429 and scheduled a retry about 482 seconds later. The 90-second harness ended the session, and further model load tests were stopped. This is an incomplete service-limited trial, not a successful Chrome observation or a collector timeout. Earlier development trials with only schema hints still had argument rejections; these are retained as failed evidence rather than included as successful results.

These small, ordered observations demonstrate reduced client decisions/calls, not a guaranteed speedup. Initial model latency did not improve consistently: the CPU first-call boundary was 4.20 s before and 12.96 s in the complete final-variant session. The interface cannot remove model/service waiting or provider rate limiting. Native tests used the final catalog/delivery behavior; subsequent ownership bookkeeping and cancellation-test hardening were verified by the final SDK/Rust suite, without another model run after the 429.

Reproduce the Python-free managed/raw integration checks against a running test application:

```powershell
node scripts/test-mcp-managed.mjs <exe> <tui-pid> <sdk-package-directory> <output.json>
node scripts/test-mcp-typescript-sdk.mjs <exe> <tui-pid> <sdk-package-directory> <raw-output.json>
node scripts/run-mcp-workflow.mjs <exe> <sdk-package-directory> scripts/mcp-workflows/file-paths.mjs <paths-output.json>
```

Final validation passed 1,207 Rust tests (11 intentionally ignored), four CLI tests, formatting and all-target/all-feature Clippy with warnings denied. New regressions cover page-chain ownership, consumed-job replay, retention opt-in, selection/cleanup failure, explicit release/log closure, bounded bridge bookkeeping, confirmation behavior, metric-enum parity and all example arguments. The previous timer-racing cancellation mock was made deterministically pending. Release build succeeded after waiting for the benchmark-owned executable lock to clear.

The maintained SDK harness completed 17 groups: cached getters/minimal metrics, process-reference survival after capture cleanup, independent pagination, raw history, Network/Threads/Scheduling/Open Files/modules/image, file-path cleanup, explicit retention, raw ownership, ten successive captures beyond the eight-slot capacity, and another client's responsiveness during a wait. Saved-log listing reported unavailable because no Recording root was configured; no new end-to-end saved-log load or application mutation was performed. Existing Rust log/operation regressions and managed log-closure tests retain that coverage. The independent TypeScript SDK 1.32.2 raw workflow negotiated MCP 2025-11-25 and passed process/metrics/files plus structured/text parity. The simplified path workflow used one MCP call, including cleanup.

The final executable was copied with a backup to the existing registration and checked for matching hashes; Live sampling remained non-stale and Recording inactive. The default lifecycle change is unreleased: retained-result clients must use raw mode or opt into retention. Both READMEs and the canonical MCP contract describe this migration. Local native/SDK evidence is retained under `notes/mcp-warm-session-20261103/`; no Python was executed during this follow-up.

## CPU latency decomposition and model retries (2026-11-03, Issue #205)

Re-audited the CPU observations after the maintainer challenged the 16-second result. **The 16.2558-second observation itself contained HTTP 429 and a scheduled 4.9983-second retry wait**, not just the later aborted Chrome turn. The earlier report did not identify this contamination clearly enough. Its measured phases were 12.9564 seconds from turn submission to the MCP request, **5.8461 ms** inside the MCP call, and 3.2936 seconds after the MCP reply until the final answer. The retry wait is part of the first interval, not an additional duration to sum. Another CPU observation without a logged retry took 14.2496 seconds: 9.0699 before MCP, 4.2146 ms in MCP, and 5.1755 after MCP. These are existing published-sample reads, not forced new CPU sampling.

The native session benchmark now supports the optional final query argument `cpu` (default remains `status`), checks the actual live-system CPU value and tool name, and records before/in/after-MCP intervals separately. Explicit `fast` tier and alternating `high`/`low` effort are available for controlled tests. Each benchmark isolates its configured MCP server map to WinProc, retaining all 26 WinProc tools; this test-only context change is not applied to the user's general MCP configuration. It is an additional difference from the earlier multi-server measurements.

The installed CLI rejects overrides of the reserved built-in `openai` provider, so provider retry settings are not changed. Instead the harness detects HTTP 429 on stderr, interrupts that exact measurement turn and retains its failed evidence, then stops further turns. Rate events are matched by turn ID as well as timestamps to avoid clock-boundary omissions. This guard was exercised: an Astra request hit 429 before any MCP call, scheduled about 270 seconds of retry delay, and was interrupted after about 197 ms. It is excluded from successful latency statistics. A read-only usage snapshot did not report an exhausted quota, but it cannot explain or rule out other throttling; no further burst on that model was attempted.

The same natural CPU request, all 26 direct tools, Astra and Fast produced five successful fresh reads before that interruption. One initial low-effort warm-up took 6.880 seconds. Subsequent alternating observations were:

| Effort | Complete turn times | Before-MCP times | In-MCP times |
| --- | --- | --- | --- |
| High, n=2 | 7.105, 4.803 s | 3.357, 2.362 s | 1.078, 0.657 ms |
| Low, n=2 | 5.659, 7.244 s | 3.232, 4.078 s | 0.661, 0.674 ms |

Lower effort did not demonstrate a stable benefit in this small sample. A separate Luna/low/Fast session with the identical natural request and MCP exposure succeeded twice, using exactly one correct metrics call per turn: first-turn completion 4.033 s (2.270 before MCP, 3.706 ms in MCP, 1.760 after), then 2.705 s (1.379 before, 1.722 ms in MCP, 1.324 after). Telemetry confirmed priority service for both model trials. These small, ordered observations support investigating a lightweight model for fixed reads, not changing the general development model automatically or claiming a guaranteed latency bound. No global model, effort or service-tier preference was changed.

Added `scripts/mcp-workflows/cpu.mjs` for the current caller to read CPU through the existing MCP connection path without starting another Codex/model session. It performs one metrics call and returns the value, sample timestamp/age, stale flag, status and warnings; unavailable values remain explicit. A fresh process using the maintained workflow runner completed connect/discovery/workflow in 77.18 ms; a PowerShell stopwatch around the **whole Node command** measured 296.77 ms with the TUI already running. This excludes the existing assistant's natural-language interpretation and final answer generation, so it is not a subsecond natural-language result. It is a programmatic MCP execution path with no Python, shell-based CPU collector or extra model request.

Reproduction:

```powershell
node scripts/run-mcp-workflow.mjs <exe> <sdk-package-directory> scripts/mcp-workflows/cpu.mjs <cpu-output.json>
node scripts/benchmark-mcp-session.mjs <codex-cli.js> <exe> <astra-output-directory> gpt-6-astra normal alternating 7 direct-only fast cpu
node scripts/benchmark-mcp-session.mjs <codex-cli.js> <exe> <luna-output-directory> gpt-6-luna normal low 2 direct-only fast cpu
```

The first command needs no model invocation. Native comparisons consume model usage; the harness stops on a rejected/rate-limited sample. Two initial harness configurations were rejected before initialization, so they made no model calls and are not latency observations. Final JavaScript syntax/diff checks, live MCP workflow execution, native CPU correctness and the rate-interruption path were verified. Rust implementation and runtime binary are unchanged from the preceding 1,207-test verification. Detailed local phase evidence is retained under `notes/mcp-warm-session-20261103/` in the CPU audit/clean-analysis files and their source traces.

## Graph-slot and CSV MCP verification (2026-11-03, Issue #206)

Implemented `winproc_get_graphs`, `winproc_add_graphs` and `winproc_export_graphs`, bringing the advertised catalog to 29 tools in managed and raw modes. System/process additions use the existing typed Graph sources; existing GPU Graphs remain discoverable/exportable. Export can append explicitly selected metrics and snapshot all registered Graphs in one action, with a destination directory, optional filename and existing all/visible/A-B ranges. See [ADR 0008](adr/0008-mcp-graph-actions-and-csv-export.md).

Final verification passed **1,220 Rust tests plus four CLI tests**, with 11 intentionally ignored tests. Formatting, all-target/all-feature Clippy with warnings denied and the statically configured Windows x64 release build passed. The 13 added Rust regressions cover append/deduplication/capacity, state preservation, stale revisions, lifetime/display-reference safety, exact CSV parity, paused/Log source provenance, inclusive ranges, validation rollback, create-new collisions/write failure, accepted-write ownership after disconnect/disable, generated-ID defaults, metric-map parity, reference limits and large encoded responses. Discovery builds a reverse reference index once per request rather than scanning the reference map for every process. A 5,000-candidate Unicode-name test verifies deterministic bounded responses and reference reuse.

The official TypeScript MCP SDK 1.32.2 negotiated MCP 2025-11-25 against an isolated, owned test TUI with an empty Graph workspace. No Python was executed. Four native groups passed:

| Group | Observed result |
|---|---|
| Combined system add/export | One MCP call added CPU and committed bytes and wrote a three-row CSV into a newly created directory containing spaces and Japanese characters. The tool call took **94.88 ms**; the complete assertion/file-read group took 95.26 ms. |
| Process Graph append | Added the test TUI's Private Bytes twice in one request; only one new slot appeared, retaining both existing Graph IDs and active selection. |
| Retry and destination collision | Reusing the original operation ID returned its original snapshot/outcome without writing again. A fresh operation targeting the existing file returned `EXPORT_FAILED`; original bytes remained unchanged. |
| Raw lifecycle and reconnect | Raw export returned a handle; a later connection could read its successful outcome. This small file had already completed before disconnect, so actual in-progress disconnect/disable survival is covered by the deterministic Rust regression, not claimed from this native observation. |

The CSV's committed-byte cell exactly matched a retained MCP metric value at the same timestamp. UTF-8/LF output, row count and absolute destination were checked. The native timing is one small warm operation, excludes natural-language/model time and is not a latency guarantee for large histories or slow storage. Recording configuration/session state remained unchanged and Live sampling stayed non-stale. A preliminary engineering run of the same combined action also succeeded before final reference-index hardening; its timing is not pooled with the final observation.

The existing managed SDK harness passed all 17 groups with the 29-tool catalog, including process references, independent pagination, metrics/history, image/files/modules/network/threads/scheduling, capture cleanup, retention, raw ownership and capacity recovery. Another client's status read during a bounded wait took 1.64 ms in that run. The independent raw SDK process/metrics/file workflow also passed. No Recording root was configured in the isolated runtime, so saved-log directory availability remained an explicit `INVALID_ARGUMENT`; no new native saved-log load is claimed. Paused/Log CSV behavior and serialization parity were verified by Rust fixtures.

Reproduce against an explicitly owned test TUI with an initially empty Graph workspace:

```powershell
node scripts/test-mcp-graph-csv.mjs <exe> <tui-pid> <sdk-package-directory> <output-directory>
```

The harness adds Graphs and creates test CSV files. It refuses to modify a nonempty starting workspace. Local detailed evidence remains in ignored `notes/`; no maintainer-specific path is part of the committed evidence. This is an unreleased local implementation of [Issue #206](https://github.com/TX230/winproc-tui/issues/206), separate from Issue #205's performance commits.
