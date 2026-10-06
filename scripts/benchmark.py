#!/usr/bin/env python3
"""Reproducible fixed-depth engine benchmark; reports speed, NOT Elo."""
import argparse
import csv
import platform
import subprocess
import time

POSITIONS=[
 ('start','rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1'),
 ('kiwipete','r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1'),
 ('rook_endgame','8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1'),
 ('tactical','rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8'),
]
def main():
 p=argparse.ArgumentParser();p.add_argument('engine');p.add_argument('--depth',type=int,default=8);p.add_argument('--threads',type=int,default=1);p.add_argument('--hash',type=int,default=64);p.add_argument('--csv',default='benchmark.csv');a=p.parse_args()
 rows=[]
 for name,fen in POSITIONS:
  proc=subprocess.Popen([a.engine],stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True,bufsize=1)
  proc.stdin.write(f'uci\nsetoption name Threads value {a.threads}\nsetoption name Hash value {a.hash}\nisready\n');proc.stdin.flush()
  for line in proc.stdout:
   if line.strip()=='readyok':break
  t=time.monotonic();proc.stdin.write(f'position fen {fen}\ngo depth {a.depth}\n');proc.stdin.flush();last={}
  for line in proc.stdout:
   fields=line.split()
   if fields[:2]==['info','depth']:
    for key in ['depth','nodes','nps','time']:
     last[key]=fields[fields.index(key)+1]
   if fields and fields[0]=='bestmove':best=fields[1];break
  seconds=time.monotonic()-t;proc.stdin.write('quit\n');proc.stdin.flush();proc.wait(timeout=10)
  row=dict(platform=platform.platform(),position=name,threads=a.threads,hash_mb=a.hash,seconds=round(seconds,4),bestmove=best,**last);rows.append(row);print(row)
 with open(a.csv,'w',newline='') as f:
  w=csv.DictWriter(f,fieldnames=rows[0].keys());w.writeheader();w.writerows(rows)
if __name__=='__main__':main()
