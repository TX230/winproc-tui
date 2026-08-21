## External Contributions

Do not open a pull request unless the maintainer has explicitly asked you to submit that specific change.
GitHub Issues are for bug reports, feedback, feature requests, and discussion only. Opening or discussing an Issue does not grant permission to submit a pull request.

Maintainer-requested or AI-assisted pull requests should fill in the sections below.

## Summary

-

## Verification

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] `cargo test`
- [ ] Manual check, if UI behavior changed:

## Documentation

- [ ] Updated `README.md` and `README.ja.md`, if user-facing overview changed.
- [ ] Updated `docs/metrics.md` and schema artifacts, if metrics, display formats, aggregation, or recording fields changed.
- [ ] Updated `docs/tracking-and-history.md`, if Tracking Lists, process identity, Ghost Rows, or Live-history retention changed.
- [ ] Updated `docs/graph-workspace.md`, if Graph, Samples, A/B, ordering, or workspace layout changed.
- [ ] Updated `docs/process-investigation.md`, if System Info or Process Info collection and lifecycle changed.
- [ ] Updated `docs/recording-and-log-view.md`, if Recording, log loading, or Log-view lifecycle changed.
- [ ] Updated `docs/architecture.md`, if cross-component responsibilities or runtime flow changed.
- [ ] Updated `scripts/package-release.ps1` and `docs/release-workflow.md` together, if packaging or publication changed.
- [ ] Checked that exact controls remain synchronized in Help, Footer, implementation, and tests.

## Related Issue (if applicable)

- Closes/Refs:
