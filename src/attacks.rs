//! Portable magic lookup tables shared by ARM64 and x86-64.
use std::sync::OnceLock;
include!(concat!(env!("OUT_DIR"), "/magics.rs"));
struct Tables {
    rook: Vec<u64>,
    bishop: Vec<u64>,
    knight: [u64; 64],
    king: [u64; 64],
    pawn: [[u64; 64]; 2],
}
static TABLES: OnceLock<Tables> = OnceLock::new();
pub fn ray(s: usize, occ: u64, bishop: bool) -> u64 {
    let dirs: &[(i32, i32)] = if bishop {
        &[(1, 1), (1, -1), (-1, 1), (-1, -1)]
    } else {
        &[(1, 0), (-1, 0), (0, 1), (0, -1)]
    };
    let mut out = 0;
    for &(df, dr) in dirs {
        let (mut f, mut r) = (s as i32 % 8 + df, s as i32 / 8 + dr);
        while (0..8).contains(&f) && (0..8).contains(&r) {
            let b = 1u64 << (r * 8 + f);
            out |= b;
            if occ & b != 0 {
                break;
            }
            f += df;
            r += dr;
        }
    }
    out
}
fn tables() -> &'static Tables {
    TABLES.get_or_init(|| {
        let mut t = Tables {
            rook: vec![0; ROOK_SIZE],
            bishop: vec![0; BISHOP_SIZE],
            knight: [0; 64],
            king: [0; 64],
            pawn: [[0; 64]; 2],
        };
        for s in 0..64 {
            for (bishop, data, dst) in [(false, &ROOK, &mut t.rook), (true, &BISHOP, &mut t.bishop)]
            {
                let (mask, magic, shift, offset) = data[s];
                let mut sub = 0u64;
                loop {
                    dst[offset + (sub.wrapping_mul(magic) >> shift) as usize] = ray(s, sub, bishop);
                    sub = sub.wrapping_sub(mask) & mask;
                    if sub == 0 {
                        break;
                    }
                }
            }
            for df in -2i32..=2 {
                for dr in -2i32..=2 {
                    let (f, r) = (s as i32 % 8 + df, s as i32 / 8 + dr);
                    if !(0..8).contains(&f) || !(0..8).contains(&r) {
                        continue;
                    }
                    let b = 1u64 << (r * 8 + f);
                    if df.abs() * dr.abs() == 2 {
                        t.knight[s] |= b;
                    }
                    if df.abs() <= 1 && dr.abs() <= 1 && (df != 0 || dr != 0) {
                        t.king[s] |= b;
                    }
                    if df.abs() == 1 && dr == 1 {
                        t.pawn[0][s] |= b;
                    }
                    if df.abs() == 1 && dr == -1 {
                        t.pawn[1][s] |= b;
                    }
                }
            }
        }
        t
    })
}
#[inline]
pub fn rook(s: usize, o: u64) -> u64 {
    let (mask, m, shift, offset) = ROOK[s];
    tables().rook[offset + ((o & mask).wrapping_mul(m) >> shift) as usize]
}
#[inline]
pub fn bishop(s: usize, o: u64) -> u64 {
    let (mask, m, shift, offset) = BISHOP[s];
    tables().bishop[offset + ((o & mask).wrapping_mul(m) >> shift) as usize]
}
#[inline]
pub fn knight(s: usize) -> u64 {
    tables().knight[s]
}
#[inline]
pub fn king(s: usize) -> u64 {
    tables().king[s]
}
#[inline]
pub fn pawn(c: usize, s: usize) -> u64 {
    tables().pawn[c][s]
}
#[inline]
pub fn pop(bb: &mut u64) -> usize {
    let s = bb.trailing_zeros() as usize;
    *bb &= *bb - 1;
    s
}
pub fn init() {
    let _ = tables();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhaustive_magics() {
        for s in 0..64 {
            for (diag, data) in [(false, &ROOK), (true, &BISHOP)] {
                let mask = data[s].0;
                let mut o = 0;
                loop {
                    assert_eq!(
                        if diag { bishop(s, o) } else { rook(s, o) },
                        ray(s, o, diag)
                    );
                    o = o.wrapping_sub(mask) & mask;
                    if o == 0 {
                        break;
                    }
                }
            }
        }
    }
}
