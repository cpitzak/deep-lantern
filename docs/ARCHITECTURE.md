# Architecture decisions

## Scope

Standard chess; original engine; Rust 2021; no Rust dependencies. Native targets
are `aarch64-apple-darwin` and `x86_64-pc-windows-msvc`. Linux is the local test
host. Native macOS/Windows linking and execution require those environments.

The engine is independent of React, Tauri and Lichess. The controller owns server
state, account credentials, move submission and human/engine handoff. It sends
complete move history to UCI so repetitions survive a handoff. FEN alone is
insufficient to reconstruct earlier repetitions.

## Board and attacks

A1=0/H8=63. Piece IDs are white P/N/B/R/Q/K then black P/N/B/R/Q/K. Twelve piece
bitboards and two occupancies coexist with a 64-entry piece array. Mutation is
centralized in remove/put; undo restores irreversible state. Test-only checks
recompute hashes and verify representation agreement.

Packed 16-bit moves contain six-bit source and destination, two promotion bits,
and two special-move bits. Promotion status follows from a pawn's last-rank move;
therefore converting a Move to UCI text requires its source position.

Portable multiply/shift magics are generated from a fixed PRNG seed at build
time. Runtime initialization fills the attack arrays once. Exhaustive tests
compare every masked occupancy with the independent ray walker. No BMI2
requirement is imposed on Apple Silicon. CPU-specific backends may be introduced
behind the existing attack interface after measurement.

Direct legality uses checkers, a single-check evasion mask, and per-square pin
rays. King safety is checked with the old king square removed from occupancy.
En passant constructs its resulting occupancy and removes the captured pawn
from attacker consideration. Castling checks both transit and destination.

Hash keys include side, pieces, castling rights and an EP file only when a legal
EP capture exists. Repetition history is distinct from the TT. Basic dead-material
recognition is intentionally conservative; it is not a general dead-position solver.

## Search and concurrency

One stdin/control thread manages search lifetime; one coordinator search worker
and optional helper workers perform lazy SMP. Helpers diversify root ordering;
only the coordinator emits analysis and selects the returned move. All workers
share a bounded TT, stop flag and aggregate node counter. Each owns its position,
PV table, killers and history. No mutable position is shared between threads.

TT slots use sequentially consistent version/key/data atomics. Readers reject
busy or changed versions. Writers skip contended slots. Bound reuse checks depth
and halfmove-clock compatibility; mate scores are normalized for ply. Null-search
subtrees do not store/reuse score bounds, and immediately repetition-sensitive
nodes avoid bound reuse. Repetition remains path-dependent: graph-history effects
are a continuing search-quality concern, not something a board hash fully solves.

Search pruning is heuristic. The current null-move guard disables consecutive
null moves, requires non-pawn material and excludes checks/PV nodes. This does not
eliminate every zugzwang case. LMR re-searches promising reduced moves. These
features require match-based validation before claiming strength gains.

The engine checks cancellation on visited nodes and hard elapsed limits at
periodic node intervals. A completed iterative-deepening result supplies the
fallback move. Even cancellation before depth one returns a legal root move.
Changing position/options joins prior workers. `isready` does not wait for search.

## Evaluation

Current evaluation is an original hand-crafted tapered evaluator. It is not
NNUE, and no random or untrained network is shipped. A future NNUE design should
choose feature encoding, architecture, quantization, accumulator ownership,
network file validation/versioning, training data and reproducible strength tests
together. ARM NEON and x86 SIMD can share one semantic evaluator contract.

## Release engineering

Rust toolchain pinned to 1.85.1. Portable default release builds do not assume
`target-cpu=native`. LTO is disabled in the delivered profile because the local
link environment produced unresolved symbols under ThinLTO. This is a build
configuration choice, not a chess algorithm restriction. Revisit LTO with native
macOS/Windows benchmarks and complete tests before changing the default.

Correctness gates and strength gates are separate. CI enforces formatting,
Clippy, Rust tests and independent move/protocol validation. Controlled engine
matches/SPRT and NNUE training are not yet automated. No Elo is claimed.

## Design references

These are algorithm references, not copied engine implementations:

- https://chessprogramming.org/Bitboards
- https://chessprogramming.org/Magic_Bitboards
- https://chessprogramming.org/Legal_Move
- https://chessprogramming.org/Perft_Results
- https://chessprogramming.org/Transposition_Table
- https://chessprogramming.org/Quiescence_Search
- https://chessprogramming.org/Search_Progression
- https://chessprogramming.org/Time_Management
- https://chessprogramming.org/UCI

## Ponder lifecycle (v0.2.0)

Each search owns a Control object distinct from its search-stop flag. The clock
uses an atomic start offset; the pondering sentinel suspends soft/hard budgets.
ponderhit atomically replaces that sentinel once. A condition variable gates
bestmove emission, so an internal node limit or completed depth cannot release
ponder output. stop/quit/EOF release the gate and join workers. The completed
principal variation supplies the predicted reply; output validates its legality.
Existing search/TT state is retained across a ponderhit, not restarted.

## Polyglot (v0.2.0)

Polyglot keys are separate from repetition/TT keys: they use the standardized
781-value random table and pseudo-capturable EP, even for pinned pawns. Files
contain sorted big-endian 16-byte records. Opening validates length and ordering
with a streaming pass; probes binary-search a mutex-protected buffered file and
scan only matching keys. Resident book memory is bounded by the read buffer.
The reader normalizes king-to-rook castling and matches all moves against legal
engine moves, including promotions, EP, and searchmoves restrictions. Learn data
is ignored. Book weights select the highest positive legal candidate; equal
weights retain file order. Files must remain immutable while open. Startup
validation is synchronous; isready is responsive during search, not book loading.

Reference: https://hgm.nubati.net/book_format.html
The standardized random constants are interoperability data; no python-chess
implementation is included. Python-chess remains a test-only dependency.
