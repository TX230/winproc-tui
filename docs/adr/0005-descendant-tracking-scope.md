# ADR 0005: Identity-scoped descendant tracking

- Status: Accepted
- Date: 2026-11-02
- Issue: [Issue #203](https://github.com/TX230/winproc-tui/issues/203)

## Context and constraints

Name-only tracking misses helper processes in application trees. Adding helper names would include unrelated processes. Tracking must survive ancestor exit, respect PID lifetimes, support overlapping roots and confirmation before history loss, and keep profile definitions reusable. Display preferences cannot define collection scope.

## Decision

Keep explicit root names in the Current Investigation and profiles. A model-owned `TrackingScope` maps full process identities to supporting root names, deriving ancestry iteratively from complete snapshots with validated parent lifetimes. Preserve observed membership after ancestor exit; remove provenance when its root is removed. Bound retained provenance with application history identities.

App owns Live scope and clones it with paused display state. Recording owns a separate scope seeded from known Live membership, freezes its root names, discovers later members, and releases exited membership after aggregation. Frame inclusion expresses recorded membership; schema-v3 field shapes remain unchanged. Log view trusts recorded members instead of filtering them by root names.

## Alternatives

- Add descendant names to the list: rejected because unrelated same-name processes would enter scope and saved profiles would accumulate incidental helpers.
- Recompute only current ancestry: rejected because surviving descendants would lose tracking when ancestors exit.
- Retain membership without root provenance: rejected because root removal and overlapping roots could not be handled accurately.
- Persist ancestry in every log: unnecessary for resource histories and totals; historical tree reconstruction remains outside this change.

## Consequences

One root can describe an application group without double-counting identities. Runtime membership needs bounded per-identity provenance and identity-aware retention. Processes or ancestry never observed by sampling cannot be recovered. Unknown start times do not establish new inherited edges. Removed roots cannot later restore historical edges that no longer exist. Existing per-name Ghost Row and generation limits still apply to descendants.

Canonical behavior: [tracking](../tracking-and-history.md#effective-tracking-scope), [recording](../recording-and-log-view.md#recording-scope), [metrics](../metrics.md#tracked-totals-and-scope), and [architecture](../architecture.md).
