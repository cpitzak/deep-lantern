//! Read-only, disk-backed standard-chess Polyglot book. No unsafe code or dependencies.
use crate::{board::*, polyglot_random::RANDOM};
use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::Mutex,
};

pub fn key(b: &Board) -> u64 {
    let mut key = 0;
    for (square, &piece) in b.squares.iter().enumerate() {
        if piece != EMPTY {
            let piece = piece as usize;
            let index = (piece % 6) * 2 + usize::from(piece < 6);
            key ^= RANDOM[index * 64 + square];
        }
    }
    for i in 0..4 {
        if b.castling & (1 << i) != 0 {
            key ^= RANDOM[768 + i];
        }
    }
    // Polyglot hashes pseudo-capturable en passant, even if the pawn is pinned.
    if let Some(ep) = b.ep {
        let attackers = crate::attacks::pawn(1 - b.side, ep) & b.pieces[b.side * 6 + PAWN];
        if attackers != 0 {
            key ^= RANDOM[772 + ep % 8];
        }
    }
    if b.side == 0 {
        key ^= RANDOM[780];
    }
    key
}

fn entry(reader: &mut impl Read) -> io::Result<(u64, u16, u16)> {
    let mut bytes = [0; 16];
    reader.read_exact(&mut bytes)?;
    Ok((
        u64::from_be_bytes(bytes[0..8].try_into().unwrap()),
        u16::from_be_bytes(bytes[8..10].try_into().unwrap()),
        u16::from_be_bytes(bytes[10..12].try_into().unwrap()),
    ))
}
fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub struct Book {
    file: Mutex<BufReader<File>>,
    count: u64,
}
impl Book {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() % 16 != 0 {
            return Err(invalid(
                "Polyglot book must be a regular file of 16-byte records",
            ));
        }
        let count = metadata.len() / 16;
        let mut file = BufReader::new(file);
        let mut previous = 0;
        for _ in 0..count {
            let (key, _, _) = entry(&mut file)?;
            if key < previous {
                return Err(invalid("Polyglot keys are not sorted"));
            }
            previous = key;
        }
        Ok(Self {
            file: Mutex::new(file),
            count,
        })
    }
    pub fn len(&self) -> u64 {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Highest positive weight among legal, allowed moves. Ties use file order.
    pub fn probe(&self, b: &Board, allowed: Option<&[Move]>) -> io::Result<Option<Move>> {
        let target = key(b);
        let mut file = self
            .file
            .lock()
            .map_err(|_| invalid("book reader lock poisoned"))?;
        let (mut low, mut high) = (0, self.count);
        while low < high {
            let mid = low + (high - low) / 2;
            file.seek(SeekFrom::Start(mid * 16))?;
            if entry(&mut *file)?.0 < target {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        file.seek(SeekFrom::Start(low * 16))?;
        let legal = b.legal_moves();
        let mut best = None;
        let mut best_weight = 0;
        for _ in low..self.count {
            let (key, raw, weight) = entry(&mut *file)?;
            if key != target {
                break;
            }
            if weight == 0 || weight <= best_weight || raw & 0x8000 != 0 {
                continue;
            }
            let from = ((raw >> 6) & 63) as usize;
            let mut to = (raw & 63) as usize;
            let promo = ((raw >> 12) & 7) as usize;
            if promo > 4 {
                continue;
            }
            // Polyglot encodes castling as king-to-rook, UCI uses king destination.
            if b.squares[from] == (b.side * 6 + KING) as u8 {
                to = match (from, to) {
                    (4, 7) => 6,
                    (4, 0) => 2,
                    (60, 63) => 62,
                    (60, 56) => 58,
                    _ => to,
                };
            }
            if let Some(&m) = legal.as_slice().iter().find(|&&m| {
                m.from() == from
                    && m.to() == to
                    && (if b.is_promotion(m) { m.promo() } else { 0 }) == promo
                    && allowed.is_none_or(|moves| moves.contains(&m))
            }) {
                best = Some(m);
                best_weight = weight;
            }
        }
        Ok(best)
    }
}
