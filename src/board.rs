//! Position state and direct legal move generation. A1 = bit 0.
use crate::attacks::{self as a, pop};
pub const PAWN: usize = 0;
pub const KNIGHT: usize = 1;
pub const BISHOP: usize = 2;
pub const ROOK: usize = 3;
pub const QUEEN: usize = 4;
pub const KING: usize = 5;
pub const EMPTY: u8 = 12;
pub const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
pub const EP: u16 = 1;
pub const CASTLE: u16 = 2;
#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct Move(pub u16);
impl Move {
    pub fn new(from: usize, to: usize, promo: usize, flag: u16) -> Self {
        Self(from as u16 | ((to as u16) << 6) | ((promo as u16) << 12) | (flag << 14))
    }
    pub fn from(self) -> usize {
        (self.0 & 63) as usize
    }
    pub fn to(self) -> usize {
        ((self.0 >> 6) & 63) as usize
    }
    pub fn promo(self) -> usize {
        ((self.0 >> 12) & 3) as usize + 1
    }
    // Promotion is inferred from the moving pawn reaching its last rank; bits 12..13 encode N/B/R/Q.
    pub fn flag(self) -> u16 {
        self.0 >> 14
    }
    pub fn uci(self, b: &Board) -> String {
        let mut s = format!("{}{}", square_name(self.from()), square_name(self.to()));
        if b.is_promotion(self) {
            s.push(['n', 'b', 'r', 'q'][self.promo() - 1]);
        }
        s
    }
}
#[derive(Clone)]
pub struct MoveList {
    pub moves: [Move; 256],
    pub len: usize,
}
impl Default for MoveList {
    fn default() -> Self {
        Self {
            moves: [Move(0); 256],
            len: 0,
        }
    }
}
impl MoveList {
    pub fn push(&mut self, m: Move) {
        self.moves[self.len] = m;
        self.len += 1;
    }
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.len]
    }
}
#[derive(Clone)]
pub struct Board {
    pub pieces: [u64; 12],
    pub occupancy: [u64; 2],
    pub squares: [u8; 64],
    pub side: usize,
    pub castling: u8,
    pub ep: Option<usize>,
    pub halfmove: u16,
    pub fullmove: u32,
    pub key: u64,
    pub history: Vec<u64>,
}
#[derive(Clone, Copy)]
pub struct Undo {
    captured: u8,
    castling: u8,
    ep: Option<usize>,
    halfmove: u16,
    fullmove: u32,
    key: u64,
    moved: u8,
}
fn random(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
fn piece_key(p: u8, s: usize) -> u64 {
    random((p as u64) * 64 + s as u64 + 1)
}
fn castle_key(c: u8) -> u64 {
    random(800 + c as u64)
}
fn side_key() -> u64 {
    random(900)
}
pub fn square_name(s: usize) -> String {
    format!(
        "{}{}",
        (b'a' + (s % 8) as u8) as char,
        (b'1' + (s / 8) as u8) as char
    )
}
pub fn parse_square(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.len() == 2 && (b'a'..=b'h').contains(&b[0]) && (b'1'..=b'8').contains(&b[1]) {
        Some((b[1] - b'1') as usize * 8 + (b[0] - b'a') as usize)
    } else {
        None
    }
}
impl Default for Board {
    fn default() -> Self {
        Self::from_fen(START).unwrap()
    }
}
impl Board {
    pub fn from_fen(fen: &str) -> Result<Self, String> {
        let fields: Vec<_> = fen.split_whitespace().collect();
        if fields.len() != 6 {
            return Err("FEN requires six fields".into());
        }
        let mut b = Self {
            pieces: [0; 12],
            occupancy: [0; 2],
            squares: [EMPTY; 64],
            side: 0,
            castling: 0,
            ep: None,
            halfmove: 0,
            fullmove: 1,
            key: 0,
            history: Vec::new(),
        };
        let rows: Vec<_> = fields[0].split('/').collect();
        if rows.len() != 8 {
            return Err("FEN requires eight ranks".into());
        }
        for (r, row) in rows.iter().enumerate() {
            let mut f = 0;
            for ch in row.chars() {
                if let Some(n) = ch.to_digit(10) {
                    if n == 0 || n > 8 {
                        return Err("invalid empty-square count".into());
                    }
                    f += n as usize;
                } else {
                    let p = "PNBRQKpnbrqk".find(ch).ok_or("invalid piece")?;
                    if f >= 8 {
                        return Err("rank too wide".into());
                    }
                    b.put(p as u8, (7 - r) * 8 + f);
                    f += 1;
                }
            }
            if f != 8 {
                return Err("rank width must be eight".into());
            }
        }
        b.side = match fields[1] {
            "w" => 0,
            "b" => 1,
            _ => return Err("invalid side".into()),
        };
        if fields[2] != "-" {
            for ch in fields[2].chars() {
                let bit = match ch {
                    'K' => 1,
                    'Q' => 2,
                    'k' => 4,
                    'q' => 8,
                    _ => return Err("invalid castling rights".into()),
                };
                if b.castling & bit != 0 {
                    return Err("duplicate castling right".into());
                }
                b.castling |= bit;
            }
        }
        b.ep = if fields[3] == "-" {
            None
        } else {
            Some(parse_square(fields[3]).ok_or("invalid en passant square")?)
        };
        b.halfmove = fields[4].parse().map_err(|_| "invalid halfmove clock")?;
        b.fullmove = fields[5].parse().map_err(|_| "invalid fullmove number")?;
        if b.fullmove == 0 {
            return Err("fullmove number starts at one".into());
        }
        for c in 0..2 {
            if b.pieces[c * 6 + KING].count_ones() != 1 {
                return Err("exactly one king per color required".into());
            }
            let pawns = b.pieces[c * 6 + PAWN].count_ones();
            let promotions: u32 = [(KNIGHT, 2), (BISHOP, 2), (ROOK, 2), (QUEEN, 1)]
                .iter()
                .map(|&(piece, initial)| {
                    b.pieces[c * 6 + piece].count_ones().saturating_sub(initial)
                })
                .sum();
            if pawns > 8 || b.occupancy[c].count_ones() > 16 || promotions + pawns > 8 {
                return Err("impossible material count".into());
            }
        }
        if (b.pieces[PAWN] | b.pieces[6 + PAWN]) & 0xff000000000000ff != 0 {
            return Err("pawn on promotion rank".into());
        }
        if a::king(b.king(0)) & b.pieces[11] != 0 {
            return Err("adjacent kings".into());
        }
        for (bit, k, r) in [(1, 4, 7), (2, 4, 0), (4, 60, 63), (8, 60, 56)] {
            if b.castling & bit != 0 {
                let c = if bit < 4 { 0 } else { 1 };
                if b.squares[k] != (c * 6 + KING) as u8 || b.squares[r] != (c * 6 + ROOK) as u8 {
                    return Err("castling rights without king/rook".into());
                }
            }
        }
        if let Some(ep) = b.ep {
            let expected = if b.side == 0 { 5 } else { 2 };
            let captured = if b.side == 0 {
                ep.checked_sub(8)
            } else {
                ep.checked_add(8)
            };
            if ep / 8 != expected
                || b.squares[ep] != EMPTY
                || captured
                    .is_none_or(|s| s >= 64 || b.squares[s] != ((1 - b.side) * 6 + PAWN) as u8)
            {
                return Err("inconsistent en passant square".into());
            }
        }
        if b.attacked(b.king(1 - b.side), b.side, b.all()) {
            return Err("side not to move is in check".into());
        }
        b.key = b.recompute_key();
        b.history.push(b.key);
        Ok(b)
    }
    pub fn fen(&self) -> String {
        let mut out = String::new();
        for r in (0..8).rev() {
            let mut n = 0;
            for f in 0..8 {
                let p = self.squares[r * 8 + f];
                if p == EMPTY {
                    n += 1;
                } else {
                    if n > 0 {
                        out.push(char::from(b'0' + n));
                        n = 0;
                    }
                    out.push("PNBRQKpnbrqk".as_bytes()[p as usize] as char);
                }
            }
            if n > 0 {
                out.push(char::from(b'0' + n));
            }
            if r > 0 {
                out.push('/');
            }
        }
        let mut rights = String::new();
        for (bit, ch) in [(1, 'K'), (2, 'Q'), (4, 'k'), (8, 'q')] {
            if self.castling & bit != 0 {
                rights.push(ch);
            }
        }
        if rights.is_empty() {
            rights.push('-');
        }
        format!(
            "{out} {} {rights} {} {} {}",
            if self.side == 0 { "w" } else { "b" },
            self.ep.map(square_name).unwrap_or("-".into()),
            self.halfmove,
            self.fullmove
        )
    }
    #[inline]
    pub fn all(&self) -> u64 {
        self.occupancy[0] | self.occupancy[1]
    }
    #[inline]
    pub fn king(&self, c: usize) -> usize {
        self.pieces[c * 6 + KING].trailing_zeros() as usize
    }
    #[inline]
    fn put(&mut self, p: u8, s: usize) {
        let bit = 1u64 << s;
        self.pieces[p as usize] |= bit;
        self.occupancy[p as usize / 6] |= bit;
        self.squares[s] = p;
        self.key ^= piece_key(p, s);
    }
    #[inline]
    fn remove(&mut self, s: usize) -> u8 {
        let p = self.squares[s];
        if p != EMPTY {
            let bit = 1u64 << s;
            self.pieces[p as usize] &= !bit;
            self.occupancy[p as usize / 6] &= !bit;
            self.squares[s] = EMPTY;
            self.key ^= piece_key(p, s);
        }
        p
    }
    pub fn attackers(&self, s: usize, c: usize, occ: u64) -> u64 {
        let o = c * 6;
        (a::pawn(1 - c, s) & self.pieces[o + PAWN])
            | (a::knight(s) & self.pieces[o + KNIGHT])
            | (a::king(s) & self.pieces[o + KING])
            | (a::bishop(s, occ) & (self.pieces[o + BISHOP] | self.pieces[o + QUEEN]))
            | (a::rook(s, occ) & (self.pieces[o + ROOK] | self.pieces[o + QUEEN]))
    }
    pub fn attacked(&self, s: usize, c: usize, occ: u64) -> bool {
        self.attackers(s, c, occ) != 0
    }
    pub fn in_check(&self) -> bool {
        self.attacked(self.king(self.side), 1 - self.side, self.all())
    }
    pub fn is_promotion(&self, m: Move) -> bool {
        self.squares[m.from()] % 6 == PAWN as u8 && (m.to() / 8 == 0 || m.to() / 8 == 7)
    }
    pub fn is_capture(&self, m: Move) -> bool {
        self.squares[m.to()] != EMPTY || m.flag() == EP
    }
    fn ep_safe(&self, from: usize, to: usize) -> bool {
        let cap = if self.side == 0 { to - 8 } else { to + 8 };
        let occ = (self.all() & !(1u64 << from) & !(1u64 << cap)) | (1u64 << to);
        self.attackers(self.king(self.side), 1 - self.side, occ) & !(1u64 << cap) == 0
    }
    fn ep_key(&self) -> u64 {
        if let Some(ep) = self.ep {
            let mut candidates = a::pawn(1 - self.side, ep) & self.pieces[self.side * 6];
            while candidates != 0 {
                let s = pop(&mut candidates);
                if self.ep_safe(s, ep) {
                    return random(1000 + (ep % 8) as u64);
                }
            }
        }
        0
    }
    pub fn recompute_key(&self) -> u64 {
        let mut key = castle_key(self.castling) ^ self.ep_key();
        if self.side == 1 {
            key ^= side_key();
        }
        for s in 0..64 {
            if self.squares[s] != EMPTY {
                key ^= piece_key(self.squares[s], s);
            }
        }
        key
    }
    fn add_pawn(list: &mut MoveList, from: usize, to: usize) {
        if to / 8 == 0 || to / 8 == 7 {
            for p in 0..4 {
                list.push(Move::new(from, to, p, 0));
            }
        } else {
            list.push(Move::new(from, to, 0, 0));
        }
    }
    pub fn legal_moves(&self) -> MoveList {
        let mut list = MoveList::default();
        let us = self.side;
        let them = 1 - us;
        let king = self.king(us);
        let occ = self.all();
        let own = self.occupancy[us];
        let enemy = self.occupancy[them] & !self.pieces[them * 6 + KING];
        let checkers = self.attackers(king, them, occ);
        let count = checkers.count_ones();
        let mut targets = a::king(king) & !own & !self.pieces[them * 6 + KING];
        while targets != 0 {
            let to = pop(&mut targets);
            if !self.attacked(to, them, occ & !(1u64 << king)) {
                list.push(Move::new(king, to, 0, 0));
            }
        }
        if count >= 2 {
            return list;
        }
        let mut pins = [u64::MAX; 64];
        let mut evasion = if count == 0 { u64::MAX } else { checkers };
        for (df, dr) in [
            (1i32, 0i32),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            let (mut f, mut r) = (king as i32 % 8 + df, king as i32 / 8 + dr);
            let mut blocker = None;
            let mut line = 0;
            while (0..8).contains(&f) && (0..8).contains(&r) {
                let s = (r * 8 + f) as usize;
                let bit = 1u64 << s;
                line |= bit;
                if own & bit != 0 {
                    if blocker.is_some() {
                        break;
                    }
                    blocker = Some(s);
                } else if self.occupancy[them] & bit != 0 {
                    let p = self.squares[s] as usize % 6;
                    let slider = p == QUEEN || p == if df == 0 || dr == 0 { ROOK } else { BISHOP };
                    if slider {
                        if let Some(b) = blocker {
                            pins[b] = line;
                        } else if count == 1 {
                            evasion = line;
                        }
                    }
                    break;
                }
                f += df;
                r += dr;
            }
        }
        let mut pawns = self.pieces[us * 6 + PAWN];
        while pawns != 0 {
            let from = pop(&mut pawns);
            let to = if us == 0 { from + 8 } else { from - 8 };
            let mask = pins[from] & evasion;
            if occ & (1u64 << to) == 0 {
                if mask & (1u64 << to) != 0 {
                    Self::add_pawn(&mut list, from, to);
                }
                if from / 8 == if us == 0 { 1 } else { 6 } {
                    let two = if us == 0 { from + 16 } else { from - 16 };
                    if occ & (1u64 << two) == 0 && mask & (1u64 << two) != 0 {
                        list.push(Move::new(from, two, 0, 0));
                    }
                }
            }
            let mut caps = a::pawn(us, from) & enemy & mask;
            while caps != 0 {
                Self::add_pawn(&mut list, from, pop(&mut caps));
            }
            if let Some(ep) = self.ep {
                if a::pawn(us, from) & (1u64 << ep) != 0 && self.ep_safe(from, ep) {
                    list.push(Move::new(from, ep, 0, EP));
                }
            }
        }
        for piece in KNIGHT..=QUEEN {
            let mut bb = self.pieces[us * 6 + piece];
            while bb != 0 {
                let from = pop(&mut bb);
                let attacks = match piece {
                    KNIGHT => a::knight(from),
                    BISHOP => a::bishop(from, occ),
                    ROOK => a::rook(from, occ),
                    _ => a::rook(from, occ) | a::bishop(from, occ),
                };
                let mut tos = attacks & !own & !self.pieces[them * 6 + KING] & pins[from] & evasion;
                while tos != 0 {
                    list.push(Move::new(from, pop(&mut tos), 0, 0));
                }
            }
        }
        if count == 0 {
            let base = us * 56;
            for (right, rook, through, to, empty) in [(1u8, 7, 5, 6, 0x60u64), (2, 0, 3, 2, 0x0e)] {
                if self.castling & (right << (us * 2)) != 0
                    && king == base + 4
                    && self.squares[base + rook] == (us * 6 + ROOK) as u8
                    && occ & (empty << base) == 0
                    && !self.attacked(base + through, them, occ & !(1u64 << king))
                    && !self.attacked(base + to, them, occ & !(1u64 << king))
                {
                    list.push(Move::new(king, base + to, 0, CASTLE));
                }
            }
        }
        list
    }
    pub fn parse_move(&self, text: &str) -> Option<Move> {
        self.legal_moves()
            .as_slice()
            .iter()
            .copied()
            .find(|m| m.uci(self) == text)
    }
    pub fn make(&mut self, m: Move) -> Undo {
        let from = m.from();
        let to = m.to();
        let moved = self.squares[from];
        let capture_sq = if m.flag() == EP {
            if self.side == 0 {
                to - 8
            } else {
                to + 8
            }
        } else {
            to
        };
        let u = Undo {
            captured: self.squares[capture_sq],
            castling: self.castling,
            ep: self.ep,
            halfmove: self.halfmove,
            fullmove: self.fullmove,
            key: self.key,
            moved,
        };
        self.key ^= castle_key(self.castling) ^ self.ep_key();
        self.ep = None;
        self.remove(capture_sq);
        self.remove(from);
        let p = if moved as usize % 6 == PAWN && (to / 8 == 0 || to / 8 == 7) {
            (self.side * 6 + m.promo()) as u8
        } else {
            moved
        };
        self.put(p, to);
        if m.flag() == CASTLE {
            let (rf, rt) = if to > from {
                (from + 3, from + 1)
            } else {
                (from - 4, from - 1)
            };
            let rook = self.remove(rf);
            self.put(rook, rt);
        }
        for s in [from, to] {
            self.castling &= match s {
                0 => !2,
                4 => !3,
                7 => !1,
                56 => !8,
                60 => !12,
                63 => !4,
                _ => 15,
            };
        }
        self.halfmove = if moved as usize % 6 == PAWN || u.captured != EMPTY {
            0
        } else {
            self.halfmove.saturating_add(1)
        };
        if moved as usize % 6 == PAWN && from.abs_diff(to) == 16 {
            self.ep = Some((from + to) / 2);
        }
        if self.side == 1 {
            self.fullmove = self.fullmove.saturating_add(1);
        }
        self.side ^= 1;
        self.key ^= side_key() ^ castle_key(self.castling) ^ self.ep_key();
        self.history.push(self.key);
        u
    }
    pub fn unmake(&mut self, m: Move, u: Undo) {
        self.history.pop();
        self.side ^= 1;
        self.remove(m.to());
        self.put(u.moved, m.from());
        if m.flag() == CASTLE {
            let (rf, rt) = if m.to() > m.from() {
                (m.from() + 3, m.from() + 1)
            } else {
                (m.from() - 4, m.from() - 1)
            };
            let rook = self.remove(rt);
            self.put(rook, rf);
        }
        if u.captured != EMPTY {
            let sq = if m.flag() == EP {
                if self.side == 0 {
                    m.to() - 8
                } else {
                    m.to() + 8
                }
            } else {
                m.to()
            };
            self.put(u.captured, sq);
        }
        self.castling = u.castling;
        self.ep = u.ep;
        self.halfmove = u.halfmove;
        self.fullmove = u.fullmove;
        self.key = u.key;
    }
    pub fn make_null(&mut self) -> (Option<usize>, u64) {
        let u = (self.ep, self.key);
        self.key ^= self.ep_key() ^ side_key();
        self.ep = None;
        self.side ^= 1;
        u
    }
    pub fn unmake_null(&mut self, u: (Option<usize>, u64)) {
        self.side ^= 1;
        self.ep = u.0;
        self.key = u.1;
    }
    pub fn repetition(&self) -> bool {
        self.history
            .iter()
            .rev()
            .take(self.halfmove as usize + 1)
            .filter(|&&k| k == self.key)
            .count()
            >= 3
    }
    pub fn insufficient(&self) -> bool {
        if self.pieces[0]
            | self.pieces[6]
            | self.pieces[3]
            | self.pieces[9]
            | self.pieces[4]
            | self.pieces[10]
            != 0
        {
            return false;
        }
        let knights = self.pieces[1] | self.pieces[7];
        let bishops = self.pieces[2] | self.pieces[8];
        if (knights | bishops).count_ones() <= 1 {
            return true;
        }
        let dark = 0xaa55aa55aa55aa55;
        knights == 0 && (bishops & dark == 0 || bishops & !dark == 0)
    }
    pub fn non_pawn_material(&self) -> bool {
        let o = self.side * 6;
        self.pieces[o + KNIGHT]
            | self.pieces[o + BISHOP]
            | self.pieces[o + ROOK]
            | self.pieces[o + QUEEN]
            != 0
    }
}
pub fn perft(b: &mut Board, depth: u8) -> u64 {
    if depth == 0 {
        return 1;
    }
    let moves = b.legal_moves();
    if depth == 1 {
        return moves.len as u64;
    }
    let mut n = 0;
    for &m in moves.as_slice() {
        let u = b.make(m);
        n += perft(b, depth - 1);
        b.unmake(m, u);
    }
    n
}
