use deep_lantern::{
    board::*,
    search::{self, Limits, Table, MATE},
};
use std::sync::{atomic::AtomicBool, Arc};
#[test]
fn published_perft_positions() {
    for (fen, counts) in [
        (START, vec![20, 400, 8902, 197281, 4865609]),
        (
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            vec![48, 2039, 97862, 4085603],
        ),
        (
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            vec![14, 191, 2812, 43238, 674624],
        ),
        (
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
            vec![6, 264, 9467, 422333],
        ),
        (
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
            vec![44, 1486, 62379, 2103487],
        ),
    ] {
        let mut b = Board::from_fen(fen).unwrap();
        for (d, n) in counts.iter().enumerate() {
            assert_eq!(perft(&mut b, (d + 1) as u8), *n, "{fen}, depth {}", d + 1);
            assert_eq!(b.fen(), fen);
        }
    }
}
#[test]
fn incremental_hash_and_undo() {
    let mut b = Board::default();
    let original = b.fen();
    let key = b.key;
    let mut stack = Vec::new();
    let mut seed = 123456789u64;
    for _ in 0..300 {
        let moves = b.legal_moves();
        if moves.len == 0 {
            break;
        }
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let m = moves.moves[seed as usize % moves.len];
        let u = b.make(m);
        assert_eq!(b.key, b.recompute_key());
        for s in 0..64 {
            let p = b.squares[s];
            let bit = 1u64 << s;
            assert_eq!(b.all() & bit != 0, p != EMPTY);
            if p != EMPTY {
                assert_ne!(b.pieces[p as usize] & bit, 0);
            }
        }
        stack.push((m, u));
    }
    while let Some((m, u)) = stack.pop() {
        b.unmake(m, u);
        assert_eq!(b.key, b.recompute_key());
    }
    assert_eq!(b.fen(), original);
    assert_eq!(b.key, key);
    assert_eq!(b.history, vec![key]);
}
#[test]
fn special_moves_restore() {
    for fen in [
        "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1",
        "4k3/P7/8/3pP3/8/8/7p/4K3 w - d6 0 1",
    ] {
        let mut b = Board::from_fen(fen).unwrap();
        let key = b.key;
        for &m in b.legal_moves().as_slice() {
            let u = b.make(m);
            assert_eq!(b.key, b.recompute_key());
            b.unmake(m, u);
            assert_eq!(b.fen(), fen);
            assert_eq!(b.key, key);
        }
    }
}
#[test]
fn en_passant_discovered_check() {
    let b = Board::from_fen("7k/8/8/r4pPK/8/8/8/8 w - f6 0 1").unwrap();
    assert!(b.parse_move("g5f6").is_none());
    let no_ep = Board::from_fen("7k/8/8/r4pPK/8/8/8/8 w - - 0 1").unwrap();
    assert_eq!(b.key, no_ep.key);
}
#[test]
fn repetition_and_null_restore() {
    let mut b = Board::default();
    for s in ["g1f3", "g8f6", "f3g1", "f6g8"] {
        b.make(b.parse_move(s).unwrap());
    }
    assert!(!b.repetition());
    for s in ["g1f3", "g8f6", "f3g1", "f6g8"] {
        b.make(b.parse_move(s).unwrap());
    }
    assert!(b.repetition());
    let key = b.key;
    let fen = b.fen();
    let u = b.make_null();
    assert_eq!(b.key, b.recompute_key());
    b.unmake_null(u);
    assert_eq!(b.fen(), fen);
    assert_eq!(b.key, key);
}
#[test]
fn dead_positions() {
    for fen in [
        "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
        "4k3/8/8/8/8/8/8/2B1K3 w - - 0 1",
    ] {
        assert!(Board::from_fen(fen).unwrap().insufficient());
    }
    assert!(!Board::from_fen("4k3/8/8/8/8/8/8/1NN1K3 w - - 0 1")
        .unwrap()
        .insufficient());
}
fn search_position(fen: &str, depth: u8, threads: usize) -> search::Outcome {
    search::run(
        Board::from_fen(fen).unwrap(),
        Limits {
            depth,
            ..Limits::default()
        },
        Arc::new(Table::new(4)),
        Arc::new(AtomicBool::new(false)),
        threads,
        |_| {},
    )
}
#[test]
fn mate_in_one() {
    let fen = "7k/8/5KQ1/8/8/8/8/8 w - - 0 1";
    for threads in [1, 2] {
        let result = search_position(fen, 3, threads);
        assert!(result.score >= MATE - 2);
        let mut b = Board::from_fen(fen).unwrap();
        b.make(result.best.unwrap());
        assert!(b.in_check());
        assert_eq!(b.legal_moves().len, 0);
    }
}
#[test]
fn terminal_positions() {
    let r = search_position("7k/6Q1/5K2/8/8/8/8/8 b - - 100 1", 3, 1);
    assert!(r.best.is_none());
    assert_eq!(r.score, -MATE);
    let r = search_position("7k/5K2/6Q1/8/8/8/8/8 b - - 0 1", 3, 1);
    assert!(r.best.is_none());
    assert_eq!(r.score, 0);
}
#[test]
fn restricted_root_and_nodes() {
    let b = Board::default();
    let only = b.parse_move("e2e4").unwrap();
    let r = search::run(
        b,
        Limits {
            depth: 4,
            nodes: Some(1000),
            root_moves: Some(vec![only]),
            ..Limits::default()
        },
        Arc::new(Table::new(1)),
        Arc::new(AtomicBool::new(false)),
        1,
        |_| {},
    );
    assert_eq!(r.best, Some(only));
    assert!(r.nodes <= 1000);
}
#[test]
fn enemy_king_blocks_pin_ray() {
    let b = Board::from_fen("rnb1kr2/p1pp2p1/1p1n1p2/B3b1Pp/2Bp4/P6P/RPP1KP2/4Q1NR b - - 3 17")
        .unwrap();
    for m in ["e5f4", "e5g3", "e5h2"] {
        assert!(b.parse_move(m).is_some());
    }
}

#[test]
fn invalid_fens_are_rejected() {
    for fen in [
        "8/8/8/8/8/8/8/8 w - - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w K - 0 1",
        "4k3/8/8/8/8/8/8/4K3 w - d6 0 1",
        "4k3/8/8/8/8/8/PPPPPPPP/QQ2K3 w - - 0 1",
    ] {
        assert!(Board::from_fen(fen).is_err(), "{fen}");
    }
}
