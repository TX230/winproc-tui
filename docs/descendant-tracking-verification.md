# Descendant tracking verification

Date: 2026-11-02. Scope: [Issue #203](https://github.com/TX230/winproc-tui/issues/203), implementation commit `7dc0cc4`.

## Code review

Reviewed identity-scoped root provenance, iterative forest traversal, parent validation, root removal and history confirmation, generation pruning, paused-display separation, Recording seeding/aggregation, Log-view membership, and MCP view publication. No actionable production-code defect was established in this review. In particular, traversal is parent-first through the complete forest, with no one-generation depth limit.

A reported failure to track grandchildren was not reproduced against this implementation. The locally configured MCP executable belonged to a different checkout that did not contain this implementation; that observation does not establish which executable produced the reported symptom.

## Native Windows system test

Run from the repository root with the Windows Rust toolchain installed:

```powershell
cargo test native_descendant_tracking_system -- --ignored --nocapture
```

The ignored test compiles `scripts/test-descendant-process.rs` into uniquely named native helpers. It runs production `App::new`, the real Windows sampling worker, application tracking/history transitions, Recording writer, log loader, and TestBackend rendering. It does not connect to the existing TUI, change user configuration, or start an MCP listener. The helper stop protocol and bounded lifetime limit cleanup to test-owned processes.

The four-generation run passed in 140.35 seconds:

- Explicit root -> child -> grandchild -> great-grandchild all tracked, with distinct executable names.
- A separate same-name worker excluded until its name was explicitly added.
- Real parent PID relationships, nonzero private-memory measurements, and totals verified.
- Tree output showed `T` for the root and `t` for every descendant; Windows-created console hosts also entered scope correctly.
- Parent exited normally; existing descendants stayed tracked, and a later child inherited tracking.
- Display paused while 125 additional real samples accumulated; descendant history exceeded general 120-sample retention.
- Five-second aggregated recording retained root-only metadata and descendants; clean end record and Log-view reload verified.
- Overlapping roots preserved membership without duplicate identity counting; removing the final root pruned history to 120 samples.
- Test helpers acknowledged normal exit; a subsequent OS process enumeration found no remaining fixture processes.

Evidence is generated beneath ignored `notes/descendant-system-<unique-name>/`: `initial-processes.txt`, `tree.txt`, `descendants.log`, `log-view.txt`, and `result.json`. Do not publish raw host snapshots with release artifacts.

## Other validation and limits

`cargo test`: 1,185 unit tests and four CLI tests passed; eleven ignored tests include the native system test run separately above. Formatting, all-target Clippy with warnings denied, debug build, and `git diff --check` passed.

This is native sampler/application system coverage, not physical Windows Terminal screenshot verification or an MCP transport test. MCP publication was inspected in-process. Forced OS PID reuse was not exercised; identity changes, invalid ancestry, cycles, and 4,096-deep traversal retain model regression coverage. Existing start-time availability and sampling visibility limitations remain in the [tracking specification](tracking-and-history.md#effective-tracking-scope).

## Visibility follow-up

The subsequent visibility adjustment uses an uppercase tracking badge for both explicit and inherited membership, distinguishes inherited membership with a gray background, and emphasizes the total row with yellow text at normal weight. Updated buffer regressions cover badge text/background and total label/metric styling; the native run above records the earlier marker appearance. Tracking and recording semantics are unchanged.
