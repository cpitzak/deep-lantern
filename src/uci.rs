//! Responsive UCI control; search runs off the stdin thread.
use crate::{
    board::*,
    search::{self, Control, Limits, Table, MATE},
};
use std::{
    io::{self, BufRead, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::Duration,
};
fn emit(s: &str) {
    let out = io::stdout();
    let mut lock = out.lock();
    let _ = writeln!(lock, "{s}");
    let _ = lock.flush();
}
struct Active {
    control: Arc<Control>,
    stop: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}
fn finish(active: &mut Option<Active>) {
    if let Some(a) = active.take() {
        a.stop.store(true, Ordering::Relaxed);
        a.control.release();
        if a.handle.join().is_err() {
            emit("info string search worker failed");
        }
    }
}
fn number(tokens: &[&str], key: &str) -> Result<Option<u64>, String> {
    if let Some(i) = tokens.iter().position(|s| *s == key) {
        Ok(Some(
            tokens
                .get(i + 1)
                .ok_or_else(|| format!("{key} requires a number"))?
                .parse()
                .map_err(|_| format!("invalid {key}"))?,
        ))
    } else {
        Ok(None)
    }
}
fn limits(tokens: &[&str], b: &Board, overhead: u64) -> Result<Limits, String> {
    if tokens.contains(&"mate") {
        return Err("go mate is not supported; use depth or movetime".into());
    }
    let mut limits = Limits::default();
    if let Some(d) = number(tokens, "depth")? {
        if d == 0 || d > 96 {
            return Err("depth must be 1..96".into());
        }
        limits.depth = d as u8;
    }
    limits.nodes = number(tokens, "nodes")?;
    if limits.nodes == Some(0) {
        return Err("nodes must be positive".into());
    }
    if let Some(i) = tokens.iter().position(|s| *s == "searchmoves") {
        let mut allowed = Vec::new();
        for t in &tokens[i + 1..] {
            if [
                "depth",
                "nodes",
                "movetime",
                "wtime",
                "btime",
                "winc",
                "binc",
                "movestogo",
                "infinite",
                "ponder",
            ]
            .contains(t)
            {
                break;
            }
            allowed.push(
                b.parse_move(t)
                    .ok_or_else(|| format!("illegal searchmove {t}"))?,
            );
        }
        if allowed.is_empty() {
            return Err("searchmoves is empty".into());
        }
        limits.root_moves = Some(allowed);
    }
    if let Some(ms) = number(tokens, "movetime")? {
        let budget = Duration::from_millis(ms.saturating_sub(overhead).max(1));
        limits.soft = Some(budget);
        limits.hard = Some(budget);
    } else if !tokens.contains(&"infinite") {
        let clock = number(tokens, if b.side == 0 { "wtime" } else { "btime" })?;
        let increment = number(tokens, if b.side == 0 { "winc" } else { "binc" })?.unwrap_or(0);
        if let Some(clock) = clock {
            let available = clock.saturating_sub(overhead).max(1);
            let mtg = number(tokens, "movestogo")?.unwrap_or(30).clamp(1, 100);
            let soft = (available / mtg + increment.saturating_mul(3) / 4).clamp(1, available);
            let hard = soft.saturating_mul(4).min(available).max(1);
            limits.soft = Some(Duration::from_millis(soft));
            limits.hard = Some(Duration::from_millis(hard));
        }
    }
    Ok(limits)
}
fn position(tokens: &[&str]) -> Result<Board, String> {
    let mut index = 1;
    let mut b = match tokens.get(index) {
        Some(&"startpos") => {
            index += 1;
            Board::default()
        }
        Some(&"fen") => {
            index += 1;
            if tokens.len() < index + 6 {
                return Err("incomplete FEN".into());
            }
            let b = Board::from_fen(&tokens[index..index + 6].join(" "))?;
            index += 6;
            b
        }
        _ => return Err("expected startpos or fen".into()),
    };
    if index < tokens.len() {
        if tokens[index] != "moves" {
            return Err("expected moves".into());
        }
        for t in &tokens[index + 1..] {
            let m = b.parse_move(t).ok_or_else(|| format!("illegal move {t}"))?;
            b.make(m);
        }
    }
    Ok(b)
}
pub fn main_loop() {
    crate::attacks::init();
    let mut board = Board::default();
    let mut table = Arc::new(Table::new(64));
    let mut threads = 1usize;
    let mut overhead = 30u64;
    let mut active: Option<Active> = None;
    let mut ponder_enabled = false;
    let mut own_book = false;
    let mut book: Option<Arc<crate::book::Book>> = None;
    let mut book_max_ply = 32u64;
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let tokens: Vec<_> = line.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        match tokens[0] {
            "uci" => {
                emit("id name Deep Lantern 0.2.0");
                emit("id author Deep Lantern contributors");
                emit("option name Hash type spin default 64 min 1 max 4096");
                emit("option name Threads type spin default 1 min 1 max 64");
                emit("option name Move Overhead type spin default 30 min 0 max 5000");
                emit("option name Clear Hash type button");
                emit("option name Ponder type check default false");
                emit("option name OwnBook type check default false");
                emit("option name BookFile type string default <empty>");
                emit("option name BookMaxPly type spin default 32 min 0 max 512");
                emit("uciok");
            }
            "isready" => emit("readyok"),
            "setoption" => {
                finish(&mut active);
                let value_idx = tokens
                    .iter()
                    .position(|t| *t == "value")
                    .unwrap_or(tokens.len());
                let name = if tokens.get(1) == Some(&"name") {
                    tokens[2..value_idx].join(" ")
                } else {
                    String::new()
                };
                let value = tokens
                    .get(value_idx + 1)
                    .and_then(|s| s.parse::<usize>().ok());
                let text_value = tokens.get(value_idx + 1..).unwrap_or(&[]).join(" ");
                match name.as_str() {
                    "Ponder" | "OwnBook" => match text_value.as_str() {
                        "true" | "false" => {
                            let enabled = text_value == "true";
                            if name == "Ponder" {
                                ponder_enabled = enabled;
                            } else {
                                own_book = enabled;
                            }
                        }
                        _ => emit("info string expected true or false"),
                    },
                    "BookFile" => {
                        // A failed replacement clears the previous book, avoiding silent stale use.
                        book = None;
                        if !text_value.is_empty() && text_value != "<empty>" {
                            match crate::book::Book::open(&text_value) {
                                Ok(loaded) => {
                                    emit(&format!(
                                        "info string book loaded: {} entries",
                                        loaded.len()
                                    ));
                                    book = Some(Arc::new(loaded));
                                }
                                Err(error) => emit(&format!(
                                    "info string book unavailable: {error}; using search"
                                )),
                            }
                        }
                    }
                    "BookMaxPly" => {
                        if let Some(v) = value.filter(|v| *v <= 512) {
                            book_max_ply = v as u64;
                        } else {
                            emit("info string BookMaxPly must be 0..512");
                        }
                    }
                    "Hash" => {
                        if let Some(v) = value.filter(|v| (1..=4096).contains(v)) {
                            table = Arc::new(Table::new(v));
                        } else {
                            emit("info string Hash must be 1..4096");
                        }
                    }
                    "Threads" => {
                        if let Some(v) = value.filter(|v| (1..=64).contains(v)) {
                            threads = v;
                        } else {
                            emit("info string Threads must be 1..64");
                        }
                    }
                    "Move Overhead" => {
                        if let Some(v) = value.filter(|v| *v <= 5000) {
                            overhead = v as u64;
                        } else {
                            emit("info string Move Overhead must be 0..5000");
                        }
                    }
                    "Clear Hash" => table.clear(),
                    _ => emit("info string unsupported option"),
                }
            }
            "ucinewgame" => {
                finish(&mut active);
                table.clear();
                board = Board::default();
            }
            "position" => {
                finish(&mut active);
                match position(&tokens) {
                    Ok(b) => board = b,
                    Err(e) => emit(&format!("info string position rejected: {e}")),
                }
            }
            "go" => {
                finish(&mut active);
                match limits(&tokens, &board, overhead) {
                    Err(e) => emit(&format!("info string go rejected: {e}")),
                    Ok(mut limits) => {
                        let control = Arc::new(Control::new(
                            tokens.contains(&"ponder"),
                            tokens.contains(&"infinite"),
                        ));
                        limits.control = Some(control.clone());
                        let worker_control = control.clone();
                        let ply =
                            u64::from(board.fullmove.saturating_sub(1)) * 2 + board.side as u64;
                        // Explicit analysis limits bypass the opening book.
                        let use_book = own_book
                            && ply < book_max_ply
                            && !tokens
                                .iter()
                                .any(|t| ["infinite", "depth", "nodes", "mate"].contains(t));
                        let worker_book = if use_book { book.clone() } else { None };
                        let stop = Arc::new(AtomicBool::new(false));
                        let worker_stop = stop.clone();
                        let tt = table.clone();
                        let b = board.clone();
                        let handle = std::thread::spawn(move || {
                            let book_move = worker_book.as_ref().and_then(|book| {
                                match book.probe(&b, limits.root_moves.as_deref()) {
                                    Ok(m) => m,
                                    Err(error) => {
                                        emit(&format!(
                                            "info string book probe failed: {error}; using search"
                                        ));
                                        None
                                    }
                                }
                            });
                            let result = if let Some(best) = book_move {
                                let mut next = b.clone();
                                next.make(best);
                                let ponder = worker_book
                                    .as_ref()
                                    .and_then(|book| book.probe(&next, None).ok().flatten());
                                emit("info string Polyglot book hit (highest weight)");
                                search::Outcome {
                                    best: Some(best),
                                    ponder,
                                    score: 0,
                                    depth: 0,
                                    nodes: 0,
                                }
                            } else {
                                search::run(b.clone(), limits, tt, worker_stop, threads, |info| {
                                    let ms = info.elapsed.as_millis() as u64;
                                    let nps = info.nodes.saturating_mul(1000) / ms.max(1);
                                    let score = if info.score.abs() > MATE - 1000 {
                                        let moves = (MATE - info.score.abs() + 1) / 2;
                                        format!(
                                            "mate {}",
                                            if info.score < 0 { -moves } else { moves }
                                        )
                                    } else {
                                        format!("cp {}", info.score)
                                    };
                                    let mut pv_board = b.clone();
                                    let mut pv = Vec::new();
                                    for m in info.pv {
                                        if !pv_board.legal_moves().as_slice().contains(&m) {
                                            break;
                                        }
                                        pv.push(m.uci(&pv_board));
                                        pv_board.make(m);
                                    }
                                    emit(&format!("info depth {} seldepth {} score {score} nodes {} nps {nps} time {ms} pv {}",info.depth,info.seldepth,info.nodes,pv.join(" ")));
                                })
                            };
                            // Even terminal positions / completed depth or node limits wait for
                            // ponderhit or stop. Internal search completion cannot release this gate.
                            worker_control.wait();
                            let mut output = format!(
                                "bestmove {}",
                                result.best.map(|m| m.uci(&b)).unwrap_or("0000".into())
                            );
                            if ponder_enabled {
                                if let (Some(best), Some(reply)) = (result.best, result.ponder) {
                                    let mut next = b.clone();
                                    next.make(best);
                                    if next.legal_moves().as_slice().contains(&reply) {
                                        output.push_str(&format!(" ponder {}", reply.uci(&next)));
                                    }
                                }
                            }
                            emit(&output);
                        });
                        active = Some(Active {
                            stop,
                            control,
                            handle,
                        });
                    }
                }
            }
            "ponderhit" => {
                if let Some(a) = &active {
                    a.control.ponderhit();
                }
            }
            "stop" => finish(&mut active),
            "quit" => {
                finish(&mut active);
                break;
            }
            "d" => emit(&format!("info string fen {}", board.fen())),
            "legal" => emit(&format!(
                "info string legal {}",
                board
                    .legal_moves()
                    .as_slice()
                    .iter()
                    .map(|m| m.uci(&board))
                    .collect::<Vec<_>>()
                    .join(" ")
            )),
            "eval" => emit(&format!(
                "info string evaluation {} cp",
                crate::eval::evaluate(&board)
            )),
            "perft" => {
                finish(&mut active);
                if let Some(d) = tokens
                    .get(1)
                    .and_then(|s| s.parse::<u8>().ok())
                    .filter(|d| *d <= 8)
                {
                    let start = std::time::Instant::now();
                    let mut sum = 0;
                    if d == 0 {
                        sum = 1;
                    } else {
                        for &m in board.legal_moves().as_slice() {
                            let text = m.uci(&board);
                            let u = board.make(m);
                            let n = perft(&mut board, d - 1);
                            board.unmake(m, u);
                            sum += n;
                            emit(&format!("info string {text}: {n}"));
                        }
                    }
                    emit(&format!(
                        "info string perft depth {d} nodes {sum} time {}",
                        start.elapsed().as_millis()
                    ));
                } else {
                    emit("info string perft depth must be 0..8");
                }
            }
            _ => {}
        }
    }
    finish(&mut active);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_bad_input() {
        assert!(position(&["position", "startpos", "moves", "e2e5"]).is_err());
        assert!(limits(&["go", "depth", "-1"], &Board::default(), 30).is_err());
        assert!(limits(&["go", "searchmoves", "e2e5"], &Board::default(), 30).is_err());
    }
    #[test]
    fn clock_budget() {
        let l = limits(
            &["go", "wtime", "1000", "winc", "100"],
            &Board::default(),
            30,
        )
        .unwrap();
        assert!(l.soft <= l.hard);
        assert!(l.hard.unwrap() <= Duration::from_millis(970));
    }
}
