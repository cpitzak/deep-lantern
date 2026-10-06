# Deep Lantern

An original Rust UCI chess engine for standard chess, with one source tree for
Apple Silicon macOS and Windows x86-64. Version 0.2.0 is playable and tested, but
has **no measured Elo**. It is not a Stockfish derivative and is not presented as
Stockfish-strength. No external Rust crates or network services are required at runtime.

## Build

Install Rust using https://rustup.rs. On macOS, install Apple's command-line
build tools (`xcode-select --install`). On Windows, use the MSVC Rust toolchain
and install Visual Studio Build Tools with **Desktop development with C++** and
a Windows SDK. The repository pins Rust 1.85.1 for reproducibility.

From this directory:

```sh
cargo test --release
cargo build --release
```

- macOS: `./target/release/deep-lantern`
- Windows PowerShell: `.\target\release\deep-lantern.exe`

The first build deterministically searches for its own magic multipliers; later
builds reuse the generated output. No copied table of magic multipliers is needed.

Add the executable as a UCI engine in a compatible chess GUI. This release is a
command-line engine; it does not include the React UI or a Lichess connector.

## Quick UCI session

Start the executable and send these commands, one per line:

```text
uci
setoption name Hash value 256
setoption name Threads value 4
isready
position startpos moves e2e4 e7e5
go movetime 3000
```

Wait for `bestmove`. For continuous analysis use `go infinite`, followed by
`stop`. `isready` remains responsive while searching. To end the process, use
`quit`. The engine also stops and joins its workers on stdin EOF.

A GUI must wait for the old search's `bestmove` after `stop` before treating
subsequent output as a new search. In manual-control mode it must discard that
old move rather than submit it to a server. UCI itself has no search IDs.

## Pondering and opening books

See [docs/LICHESS_BOT.md](docs/LICHESS_BOT.md) for the config.yml changes.
Rebuild and replace the executable used by lichess-bot before enabling pondering.

```text
setoption name Ponder value true
setoption name OwnBook value true
setoption name BookFile value /absolute/path/Cerebellum_Light_Poly.bin
setoption name BookMaxPly value 64
```

Do not quote a path in a raw UCI command; YAML paths should be quoted. Relative
paths resolve from the engine process's working directory. Book files stay open
read-only; replace/reload them only between searches. Loading validates record
length and key ordering once; disk-backed binary search avoids loading a large
book into RAM. An unavailable or invalid replacement clears the previous book,
reports a diagnostic, and falls back to normal search.

Highest positive weight wins, with file-order tie breaking. Weight is a book
preference, not a universal evaluation score or proof of strength. Illegal and
zero-weight entries are skipped. `searchmoves` is respected; explicit `depth`,
`nodes`, and `infinite` analysis bypass the book. BookMaxPly counts half-moves
from the FEN fullmove number, not the number of moves received in the command.
No third-party strength book is bundled. Cerebellum's official download could
not be fetched in this environment; compatibility was tested with independent
fixtures and python-chess's public performance.bin book instead.

The controller starts `go ponder` on the position **after the predicted opponent
reply**. The engine continues that search on `ponderhit`, starting its time
budget at the hit. On a different opponent move, the controller sends `stop`,
consumes/discards the old `bestmove`, supplies the actual position, and starts a
new search. Pondering never changes the board automatically. The engine emits
`bestmove ... ponder ...` when Ponder is enabled and a legal predicted reply is
available. At depth/node limits or terminal positions it parks until hit/stop,
rather than emitting an early move. A book hit may similarly park without using
CPU. The Ponder option controls reply output; explicit `go ponder` is always
honored. `go infinite` also waits for stop even after a depth limit completes.

## Implemented

- Twelve piece bitboards, color occupancy, and a square-to-piece array.
- Portable magic-bitboard sliding attacks; precomputed pawn/knight/king attacks.
- Direct legal move generation using checks, pins, and evasion masks.
- All four promotions, castling, and en passant, including discovered-check legality.
- Make/unmake with undo state and incremental 64-bit Zobrist hashing.
- Legally relevant en passant hashing and full move-history repetition checks.
- Threefold repetition, fifty-move handling, basic dead-material detection,
  checkmate and stalemate; mate takes precedence over the fifty-move threshold.
- Iterative deepening, fail-soft alpha-beta/PVS, aspiration windows, quiescence
  with check evasions, mate-distance pruning, null-move pruning and LMR.
- TT move ordering, MVV-LVA capture ordering, killer moves and bounded history.
- Bounded shared transposition table with depth/bound/mate-score handling and
  nonblocking versioned atomic slots. No unsafe Rust.
- Lazy SMP: configurable independent search workers sharing the TT and stop state.
- Original tapered classical evaluation: material, positional terms, mobility,
  pawn structure, passed pawns, rook files, bishop pair and king safety.
- Asynchronous UCI input and cooperative cancellation; soft and hard time budgets.
- `go depth`, `nodes`, `movetime`, clocks/increments, `movestogo`, `infinite`,
  `searchmoves`, `ponder` and `ponderhit`.
- Standard Polyglot `.bin` opening books with highest-weight legal move selection.

## Options and diagnostics

| UCI option | Default | Range |
|---|---:|---:|
| Hash (MiB, approximate table allocation) | 64 | 1–4096 |
| Threads | 1 | 1–64 |
| Move Overhead (milliseconds) | 30 | 0–5000 |
| Clear Hash | button | — |
| Ponder | false | true/false |
| OwnBook | false | true/false |
| BookFile | &lt;empty&gt; | Path to a Polyglot .bin file |
| BookMaxPly | 32 | 0–512 half-moves (0 disables probing) |

Large Hash values require sufficient free RAM. Each worker also has its own
search stack and heuristics. Test 12 and 24 threads on the Ryzen rather than
assuming 24 is stronger; test several thread counts on the M4 as well.

Additional non-UCI diagnostic commands:

- `d`: current FEN.
- `legal`: all legal moves in coordinate notation.
- `eval`: static evaluation in centipawns, side-to-move relative.
- `perft 5`: legal leaf counts and per-root-move breakdown (depths 0–8).

Perft intentionally does not stop for repetition or fifty-move draws.

## Verification

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --release
python -m pip install -r scripts/requirements-test.txt
python scripts/validate.py target/release/deep-lantern --positions 5000
python scripts/validate_features.py target/release/deep-lantern
```

On Windows, append `.exe` to the executable path. Python and python-chess are
**test dependencies only**; the executable does not require them.

The independent validator compares complete legal move sets and FENs with
python-chess, and checks UCI responsiveness, stop, timed search, root restrictions
and multiple threads. See `docs/VALIDATION.md` for the actual verification performed.
The GitHub Actions workflow runs native builds/tests on Linux, Windows and macOS
and uploads executables when the repository is pushed. No workflow has been
remotely run as part of creating this archive.

## Compare your computers

Run on each machine, with the same source revision, depth, Hash and thread count:

```sh
python scripts/benchmark.py target/release/deep-lantern --depth 8 --threads 1 --hash 256 --csv single.csv
python scripts/benchmark.py target/release/deep-lantern --depth 8 --threads 4 --hash 256 --csv parallel.csv
```

Use `.exe` on Windows. Also compare the best sustainable thread count on each
computer. Close unrelated CPU-heavy applications. Repeat measurements; exclude
build time. Fixed-depth results measure throughput/search behavior, not Elo.
Parallel search has nondeterministic node counts; NPS alone is not a strength test.

## Scope and next strength work

This is a complete playable engine, not the completed high-strength research
program. No trained NNUE model, NNUE inference backend, Syzygy probe, PEXT/SIMD
backend, Chess960, MultiPV, or `go mate` is included. Unsupported `go mate`
requests are explicitly rejected.

The next major strength milestone is an NNUE design plus legitimately sourced
training data and a validated trained network. Other candidates are SEE-based
capture ordering/pruning, richer history heuristics, improved time management
and measured CPU-specific optimizations. Each needs correctness checks and
controlled strength matches. In particular, null-move pruning/LMR are heuristic
and have not passed a statistical Elo-gain gate against an ablated baseline.

Opening books can now be selected inside the engine, or by the external controller.
Enable only one book owner. Book hits emit an `info string` diagnostic. The engine can be
connected to an authorized Lichess BOT account through a separate UCI bridge;
this project does not log in, create accounts, or play online automatically.

For experiments with opening books or tablebases, measure intrinsic engine
strength separately so external knowledge does not conceal regressions.

## Project layout

- `build.rs`: deterministic magic-number generation.
- `src/attacks.rs`: attack lookup tables and ray reference implementation.
- `src/board.rs`: state, hashing, rules, legal move generation and perft.
- `src/eval.rs`: classical evaluation.
- `src/book.rs`, `src/polyglot_random.rs`: disk-backed Polyglot reader and standard hash constants.
- `src/search.rs`: search, TT, time/node limits and parallel workers.
- `src/uci.rs`: protocol and worker lifecycle.
- `tests/correctness.rs`: perft, special-move, hash, draw and search tests.
- `scripts/`: independent validation and hardware benchmark tools.
- `docs/`: architecture decisions and recorded validation.
