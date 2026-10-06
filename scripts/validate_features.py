#!/usr/bin/env python3
"""Independent Polyglot/UCI lifecycle tests. No online account required."""
import argparse
import pathlib
import queue
import random
import struct
import tempfile
import time
import chess
import chess.engine
import chess.polyglot
from validate import Engine


def quiet(e, duration=0.15):
    deadline = time.monotonic() + duration
    while time.monotonic() < deadline:
        try:
            line = e.lines.get(timeout=max(0.001, deadline - time.monotonic()))
        except queue.Empty:
            break
        assert not line.startswith('bestmove'), ('premature bestmove', line)


def sync(e):
    e.send('isready')
    e.until('readyok')


def legal_result(line, board):
    parts = line.split()
    move = chess.Move.from_uci(parts[1])
    if board.is_game_over() and not list(board.legal_moves):
        assert parts[1] == '0000'
        return
    assert move in board.legal_moves, (line, board.fen())
    if len(parts) > 2:
        assert parts[2] == 'ponder'
        board = board.copy()
        board.push(move)
        assert chess.Move.from_uci(parts[3]) in board.legal_moves


def ponder_tests(path):
    for threads in (1, 4):
        e = Engine(path)
        try:
            e.send('uci'); e.until('uciok')
            e.send(f'setoption name Threads value {threads}')
            e.send('setoption name Ponder value true')
            e.send('setoption name Move Overhead value 0')
            for limit in ('depth 2', 'nodes 100', 'movetime 160'):
                e.send('position startpos')
                e.send('go ponder ' + limit)
                quiet(e, 0.25)
                sync(e)  # responsive even if search has already completed
                start = time.monotonic()
                e.send('ponderhit')
                line = e.until('bestmove ', 5)
                elapsed = time.monotonic() - start
                legal_result(line, chess.Board())
                if limit.startswith('movetime'):
                    assert elapsed >= 0.07, ('ponder time spent from clock', elapsed)
                quiet(e, 0.03)
            # A terminal position must not emit early either.
            e.send('position fen 7k/6Q1/5K2/8/8/8/8/8 b - - 0 1')
            e.send('go ponder depth 2'); quiet(e)
            e.send('ponderhit'); assert e.until('bestmove ') == 'bestmove 0000'
            # Immediate cancellation, repeated hits, infinite output gating, and reuse.
            for _ in range(5):
                e.send('position startpos'); e.send('go ponder wtime 2000 btime 2000')
                e.send('stop'); legal_result(e.until('bestmove '), chess.Board())
                quiet(e, 0.01)
            e.send('position startpos'); e.send('go infinite depth 1'); quiet(e)
            e.send('ponderhit'); quiet(e)  # not a ponder search: no effect
            e.send('stop'); legal_result(e.until('bestmove '), chess.Board())
            e.send('position startpos'); e.send('go ponder movetime 160')
            e.send('ponderhit'); e.send('ponderhit')
            legal_result(e.until('bestmove '), chess.Board())
            e.send('position startpos'); e.send('go depth 4')
            line = e.until('bestmove '); assert ' ponder ' in line
            legal_result(line, chess.Board())
            e.send('go ponder depth 1'); quiet(e)
        finally:
            e.close()  # quit must wake the parked worker
    # Actual client used by lichess-bot: hit and miss across consecutive play() calls.
    with chess.engine.SimpleEngine.popen_uci(path, timeout=10) as client:
        board = chess.Board()
        first = client.play(board, chess.engine.Limit(time=0.12), ponder=True)
        assert first.ponder is not None
        board.push(first.move); board.push(first.ponder)
        second = client.play(board, chess.engine.Limit(time=0.12), ponder=True)
        assert second.move in board.legal_moves
        board.push(second.move)
        reply = next(m for m in board.legal_moves if m != second.ponder)
        board.push(reply)
        third = client.play(board, chess.engine.Limit(time=0.12), ponder=False)
        assert third.move in board.legal_moves
    print('PASS: pondering hit/miss/stop/quit, clock reset, output gating, 1/4 threads, python-chess client')


def encode(board, move):
    to = move.to_square
    if board.is_castling(move):
        to = chess.square(7 if chess.square_file(to) > 4 else 0, chess.square_rank(to))
    return to | move.from_square << 6 | ((move.promotion - 1) if move.promotion else 0) << 12


def book_tests(path, external):
    rng = random.Random(701)
    positions = [chess.Board(fen) for fen in [
        chess.STARTING_FEN,
        'r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1',
        'r3k2r/8/8/8/8/8/8/R3K2R b KQkq - 0 1',
        '7k/P7/8/8/8/8/8/7K w - - 0 1',
        '7k/8/8/8/8/8/p7/7K b - - 0 1',
        '7k/8/8/3pP3/8/8/8/7K w - d6 0 1',
        '4r2k/8/8/3pP3/8/8/8/4K3 w - d6 0 1',
    ]]
    b = chess.Board()
    for i in range(1000):
        if b.is_game_over() or i % 120 == 0: b = chess.Board()
        b.push(rng.choice(list(b.legal_moves)))
        if not b.is_game_over(): positions.append(b.copy())
    records = []
    for b in positions:
        key = chess.polyglot.zobrist_hash(b)
        for move in b.legal_moves:
            records.append((key, encode(b, move), rng.randint(1, 60000), 0))
        # Zero-weight/deleted and illegal high-weight records must be ignored.
        records.extend([(key, 0, 65535, 0), (key, 0xffff, 65535, 0)])
    records.sort(key=lambda x: x[0])
    with tempfile.TemporaryDirectory(prefix='deep lantern books ') as tmp:
        book = pathlib.Path(tmp) / 'fixture with spaces.bin'
        book.write_bytes(b''.join(struct.pack('>QHHI', *r) for r in records))
        e = Engine(path)
        try:
            e.send('setoption name OwnBook value true')
            e.send('setoption name BookMaxPly value 512')
            e.send(f'setoption name BookFile value {book}'); sync(e)
            with chess.polyglot.open_reader(book) as reader:
                for b in positions:
                    expected = reader.find(b).move.uci()
                    e.send('position fen ' + b.fen(en_passant='fen'))
                    e.send('go movetime 20')
                    assert e.until('bestmove ').split()[1] == expected, b.fen()
            # All castling directions and promotion types forced by a single-entry book.
            specials = 0
            for b in positions[1:6]:
                for move in list(b.legal_moves):
                    if not (b.is_castling(move) or move.promotion or b.is_en_passant(move)): continue
                    one = pathlib.Path(tmp) / f'special-{specials}.bin'
                    one.write_bytes(struct.pack('>QHHI', chess.polyglot.zobrist_hash(b), encode(b, move), 1, 0))
                    e.send(f'setoption name BookFile value {one}'); sync(e)
                    e.send('position fen ' + b.fen(en_passant='fen')); e.send('go movetime 20')
                    assert e.until('bestmove ').split()[1] == move.uci()
                    specials += 1
            e.send(f'setoption name BookFile value {book}'); sync(e)
            e.send('position startpos'); e.send('go movetime 20 searchmoves a2a3')
            assert e.until('bestmove ').split()[1] == 'a2a3'
            e.send('go ponder movetime 20'); quiet(e)
            e.send('ponderhit'); legal_result(e.until('bestmove '), chess.Board())
            # Explicit analysis bypasses book.
            e.send('go depth 2'); e.until('info depth 2'); legal_result(e.until('bestmove '), chess.Board())
            e.send('setoption name BookMaxPly value 0')
            e.send('go movetime 100'); e.until('info depth '); e.until('bestmove ')
            e.send('setoption name BookMaxPly value 512')
            for name, data in [('empty', b''), ('truncated', b'bad'),
                               ('unsorted', struct.pack('>QHHI', 2, 0, 0, 0) + struct.pack('>QHHI', 1, 0, 0, 0)),
                               ('miss', struct.pack('>QHHI', 0, 0, 1, 0)),
                               ('zero', struct.pack('>QHHI', chess.polyglot.zobrist_hash(chess.Board()), encode(chess.Board(), chess.Move.from_uci('e2e4')), 0, 0))]:
                invalid = pathlib.Path(tmp) / (name + '.bin'); invalid.write_bytes(data)
                e.send(f'setoption name BookFile value {invalid}'); sync(e)
                e.send('go movetime 100'); e.until('info depth '); legal_result(e.until('bestmove '), chess.Board())
            e.send(f'setoption name BookFile value {tmp}/missing.bin'); sync(e)
            e.send('go movetime 100'); e.until('info depth '); e.until('bestmove ')
            if external:
                e.send(f'setoption name BookFile value {pathlib.Path(external).resolve()}'); sync(e)
                count = 0
                with chess.polyglot.open_reader(external) as reader:
                    b = chess.Board()
                    while count < 80:
                        entries = list(reader.find_all(b))
                        if not entries: break
                        expected = max(entries, key=lambda x: x.weight).move
                        e.send('position fen ' + b.fen()); e.send('go movetime 20')
                        assert e.until('bestmove ').split()[1] == expected.uci()
                        b.push(expected); count += 1
                assert count > 0
                print(f'PASS: external book {pathlib.Path(external).name}: {count} consecutive matching book moves')
        finally:
            e.close()
    print(f'PASS: {len(positions)} independent Polyglot comparisons, {specials} special moves, malformed/missing/empty books, root restriction, ponder/book interaction')


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('engine'); p.add_argument('--book')
    args = p.parse_args()
    ponder_tests(args.engine)
    book_tests(args.engine, args.book)
