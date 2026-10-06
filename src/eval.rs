//! Original tapered classical evaluation. Scores are centipawns, side-to-move relative.
//! Deliberately no untrained neural network masquerading as an NNUE evaluator.
use crate::{
    attacks::{self as a, pop},
    board::*,
};
pub const VALUES: [i32; 6] = [100, 320, 335, 500, 950, 0];
const EG: [i32; 6] = [120, 305, 330, 525, 940, 0];
const PHASE: [i32; 6] = [0, 1, 1, 2, 4, 0];
pub fn evaluate(b: &Board) -> i32 {
    let (mut mg, mut eg, mut phase) = (0, 0, 0);
    let occ = b.all();
    for c in 0..2 {
        let sign = if c == 0 { 1 } else { -1 };
        let own = b.occupancy[c];
        let pawns = b.pieces[c * 6];
        let enemy_pawns = b.pieces[(1 - c) * 6];
        for p in 0..6 {
            let mut bb = b.pieces[c * 6 + p];
            while bb != 0 {
                let s = pop(&mut bb);
                let f = (s % 8) as i32;
                let r = if c == 0 { s / 8 } else { 7 - s / 8 } as i32;
                let center = 7 - ((2 * f - 7).abs() + (2 * r - 7).abs()) / 2;
                let (mut m, mut e) = (VALUES[p], EG[p]);
                phase += PHASE[p];
                match p {
                    PAWN => {
                        m += r * 7 + center * 3;
                        e += r * 12 + center * 2;
                        let file = 0x0101010101010101u64 << (s % 8);
                        let adjacent = ((file & !0x0101010101010101) >> 1)
                            | ((file & !0x8080808080808080) << 1);
                        if pawns & adjacent == 0 {
                            m -= 14;
                            e -= 18;
                        }
                        if (pawns & file).count_ones() > 1 {
                            m -= 10;
                            e -= 16;
                        }
                        let front = if c == 0 {
                            u64::MAX.checked_shl(((s / 8 + 1) * 8) as u32).unwrap_or(0)
                        } else {
                            (1u64 << (s / 8 * 8)) - 1
                        };
                        if enemy_pawns & (file | adjacent) & front == 0 {
                            m += r * r * 2;
                            e += r * r * 5;
                            let k = b.king(1 - c);
                            let distance =
                                (k % 8).abs_diff(s % 8).max((k / 8).abs_diff(s / 8)) as i32;
                            e += distance * r;
                        }
                        if a::pawn(1 - c, s) & pawns != 0 {
                            m += 8;
                            e += 12;
                        }
                    }
                    KNIGHT => {
                        m += center * 10;
                        e += center * 7;
                        let n = (a::knight(s) & !own).count_ones() as i32;
                        m += (n - 4) * 4;
                        e += (n - 4) * 3;
                    }
                    BISHOP => {
                        m += center * 5;
                        e += center * 4;
                        let n = (a::bishop(s, occ) & !own).count_ones() as i32;
                        m += (n - 6) * 4;
                        e += (n - 6) * 4;
                    }
                    ROOK => {
                        let file = 0x0101010101010101u64 << (s % 8);
                        if pawns & file == 0 {
                            m += 14;
                            if enemy_pawns & file == 0 {
                                m += 12;
                            }
                        }
                        if r == 6 {
                            m += 18;
                            e += 25;
                        }
                        let n = (a::rook(s, occ) & !own).count_ones() as i32;
                        m += (n - 7) * 2;
                        e += (n - 7) * 4;
                    }
                    QUEEN => {
                        let n = ((a::rook(s, occ) | a::bishop(s, occ)) & !own).count_ones() as i32;
                        m += n - 12;
                        e += center * 3 + (n - 12) * 2;
                    }
                    KING => {
                        m -= center * 8 + r * 8;
                        e += center * 12;
                        let shield = a::king(s) & pawns;
                        m += shield.count_ones() as i32 * 12;
                        let mut zone = a::king(s);
                        while zone != 0 {
                            let q = pop(&mut zone);
                            if b.attacked(q, 1 - c, occ) {
                                m -= 12;
                            }
                        }
                    }
                    _ => unreachable!(),
                }
                mg += sign * m;
                eg += sign * e;
            }
        }
        if b.pieces[c * 6 + BISHOP].count_ones() >= 2 {
            mg += sign * 28;
            eg += sign * 45;
        }
    }
    let phase = phase.min(24);
    let score = (mg * phase + eg * (24 - phase)) / 24;
    (if b.side == 0 { score } else { -score }) + 12
}
