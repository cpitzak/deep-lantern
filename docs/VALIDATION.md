# Validation record — version 0.2.0

Executed on Linux x86-64 using Rust 1.85.1:

- `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`: passed.
- `cargo test --release`: **18 tests passed** (the original 14 plus 4 new tests).
- `scripts/validate.py --positions 1000`: **1,000** independent legal-move/FEN
  comparisons and existing UCI protocol checks passed.
- `scripts/validate_features.py`: pondering with 1 and 4 workers passed,
  including no premature bestmove, depth/node completion, time budget starting
  at ponderhit, repeated hit, stop, quit, terminal positions, infinite gating,
  legal predicted replies, and actual python-chess play() hit/miss transitions.
- **1,007** independently generated Polyglot position probes matched
  python-chess; **13** special-move cases covered four castlings, eight
  promotions and en passant. Separate hash tests cover pinned en passant.
- Book/ponder interaction, root restriction, analysis bypass, depth cap,
  malformed/unsorted/empty/missing/zero-weight books and misses passed.
- External `performance.bin`: **29 consecutive book moves** matched
  python-chess's highest-weight selection.
- `cargo check --all-targets --target aarch64-apple-darwin`: passed.
- `cargo check --all-targets --target x86_64-pc-windows-msvc`: passed.

External test book source (not redistributed in this archive):
https://raw.githubusercontent.com/niklasf/python-chess/master/data/polyglot/performance.bin

SHA-256: `9d30cc920d1368cbee1bbe0658dedff1091592894f3ccf0fc013a1c1ff5ebfbe`

Cerebellum 3 Merge's official download returned HTTP 403, so the exact
Cerebellum file remains untested. Native execution on M4 macOS and Windows 11
was not available; cross-target checks are not native tests. No online Lichess
games were started, no user's local config was modified, and no Elo/strength
gain or hardware performance claim is made. Native CI includes the new tests.
No third-party book is bundled. The source still has no Rust dependencies.

---

## Historical v0.1.0 validation


## Executed in the build environment

Host: Linux x86-64. Toolchain: Rust 1.85.1. No Rust crate dependencies.

- `cargo test --release`: **14 tests passed** (3 unit, 11 integration).
- Debug tests passed before the final addition of FEN material-count guards;
  the final source was then covered by the release suite and Clippy.
- `cargo clippy --all-targets -- -D warnings`: passed.
- Rustfmt applied; source uses standard formatting.
- Exhaustive magic-table comparisons against ray walking: passed.
- Five published perft suites: passed, including start-position depth 5
  (4,865,609), Kiwipete depth 4 (4,085,603), and promotion/castling/endgame cases.
- Incremental hash, undo, null-move restoration, en passant, repetition,
  conservative dead-material checks, terminal positions, mate-in-one,
  node limits, root restriction and malformed FEN checks: passed.
- Independent python-chess validator: **5,000 positions passed** for complete
  legal move sets and FEN after replaying the full move history.
- Protocol validator: readiness during search, stop, 200 ms searches, and legal
  best moves passed with 1 and 4 threads; restricted root search passed.
- Fixed-depth benchmark: all four suite positions completed at depth 7.

The independent validator found a pin-scanning defect where an enemy king did
not terminate a ray. This was fixed and captured by a dedicated regression test.
The complete 5,000-position run was repeated successfully after the final edits.

## Cross-platform checks

`cargo check --all-targets` passed for:

- `aarch64-apple-darwin` — Apple Silicon Mac mini.
- `x86_64-pc-windows-msvc` — Ryzen Windows 11 PC.

These checks compile/type-check target code; they do **not** constitute native
linking or execution. The GitHub Actions workflow is provided for native testing
and executable artifacts, but has not been run remotely. The archive contains
source, not prebuilt macOS or Windows executables.

## Limits of the evidence

- No Elo estimate, engine tournament or SPRT result exists for this release.
- No real M4/9900X performance comparison has been performed.
- Parallel correctness smoke tests are not proof of strength scaling.
- No trained NNUE network or SIMD/PEXT backend is shipped.
- Passing perft does not establish draw-rule or search correctness by itself;
  separate tests cover the listed cases, not all possible chess positions.
- Tablebase probing, general dead-position proof, opening books, Chess960,
  Lichess connectivity and a graphical UI are outside this release.

Use the included benchmark script on both computers. Use controlled matches
against fixed baselines before labeling later changes strength improvements.

## Deep Lantern rename verification

All 14 Rust tests passed after renaming the package, crate, executable and UCI
identity. The source archive was checked for the previous personal name; no
occurrences remain in its paths or file contents.
