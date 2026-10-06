#!/usr/bin/env python3
"""Independent move-generation and UCI tests. Requires python-chess (test-only)."""
import argparse
import queue
import random
import subprocess
import threading
import time
import chess

class Engine:
    def __init__(self,path):
        self.p=subprocess.Popen([path],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,bufsize=1)
        self.lines=queue.Queue()
        def read():
            for line in self.p.stdout:
                self.lines.put(line.strip())
        self.reader=threading.Thread(target=read,daemon=True)
        self.reader.start()
    def send(self,line):
        self.p.stdin.write(line+'\n'); self.p.stdin.flush()
    def until(self,prefix,timeout=15):
        deadline=time.monotonic()+timeout
        while True:
            line=self.lines.get(timeout=max(0.01,deadline-time.monotonic()))
            if line.startswith(prefix):return line
            if 'rejected' in line or 'failed' in line:raise AssertionError(line)
    def close(self):
        self.send('quit');self.p.wait(timeout=10)
        assert self.p.returncode==0

def main():
    parser=argparse.ArgumentParser();parser.add_argument('engine');parser.add_argument('--positions',type=int,default=2500);args=parser.parse_args()
    e=Engine(args.engine)
    try:
        e.send('uci');e.until('uciok');e.send('isready');e.until('readyok')
        rng=random.Random(20261005);b=chess.Board();checked=0
        for i in range(args.positions):
            if b.is_game_over() or i%150==0:b=chess.Board()
            if i%150==0:
                e.send('position startpos')
            else:
                e.send('position startpos moves '+' '.join(m.uci() for m in b.move_stack))
            e.send('legal');line=e.until('info string legal')
            actual=set(line.split()[3:]);expected={m.uci() for m in b.legal_moves}
            assert actual==expected,(b.fen(),actual-expected,expected-actual)
            e.send('d');fen=e.until('info string fen ')[16:]
            assert fen==b.fen(en_passant='fen'),(fen,b.fen(en_passant='fen'))
            moves=list(b.legal_moves)
            if moves:b.push(rng.choice(moves))
            checked+=1
        for threads in (1,4):
            e.send(f'setoption name Threads value {threads}')
            e.send('position startpos');e.send('go infinite');time.sleep(.05)
            t=time.monotonic();e.send('isready');e.until('readyok',2);assert time.monotonic()-t<2
            t=time.monotonic();e.send('stop');line=e.until('bestmove ',3);assert time.monotonic()-t<3
            assert chess.Move.from_uci(line.split()[1]) in chess.Board().legal_moves
            e.send('position startpos');t=time.monotonic();e.send('go movetime 200');line=e.until('bestmove ',3)
            assert time.monotonic()-t<2
            assert chess.Move.from_uci(line.split()[1]) in chess.Board().legal_moves
        e.send('position startpos');e.send('go depth 4 searchmoves e2e4');assert e.until('bestmove ',30)=='bestmove e2e4'
        print(f'PASS: {checked} independent legal-move/FEN comparisons; UCI readiness, stop, timed search and root restriction; 1/4 threads.')
    finally:e.close()
if __name__=='__main__':main()
