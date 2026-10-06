use deep_lantern::{board::Board, book, search::Control};
use std::{
    sync::{mpsc, Arc},
    time::Duration,
};

#[test]
fn ponder_clock_and_gate_are_independent_of_search_completion() {
    let control = Arc::new(Control::new(true, false));
    assert!(control.elapsed().is_none());
    let (tx, rx) = mpsc::channel();
    let worker = control.clone();
    let handle = std::thread::spawn(move || {
        worker.wait();
        tx.send(()).unwrap();
    });
    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
    control.ponderhit();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    handle.join().unwrap();
    let before = control.elapsed().unwrap();
    control.ponderhit();
    assert!(control.elapsed().unwrap() >= before); // repeated hit cannot reset time
}

#[test]
fn infinite_requires_stop_not_ponderhit() {
    let control = Arc::new(Control::new(false, true));
    let (tx, rx) = mpsc::channel();
    let worker = control.clone();
    let handle = std::thread::spawn(move || {
        worker.wait();
        tx.send(()).unwrap();
    });
    control.ponderhit();
    assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
    control.release();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    handle.join().unwrap();
}

#[test]
fn polyglot_standard_vectors() {
    let mut b = Board::default();
    assert_eq!(book::key(&b), 0x463b96181691fc9c);
    for (move_text, expected) in [
        ("e2e4", 0x823c9b50fd114196),
        ("d7d5", 0x0756b94461c50fb0),
        ("e4e5", 0x662fafb965db29d4),
        ("f7f5", 0x22a48b5a8e47ff78),
    ] {
        b.make(b.parse_move(move_text).unwrap());
        assert_eq!(book::key(&b), expected);
    }
}

#[test]
fn polyglot_ep_is_pseudolegal_not_legal() {
    let with_ep = Board::from_fen("4r2k/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    let without_ep = Board::from_fen("4r2k/8/8/3pP3/8/8/8/4K3 w - - 0 1").unwrap();
    assert_eq!(with_ep.key, without_ep.key); // engine repetition hash
    assert_ne!(book::key(&with_ep), book::key(&without_ep));
    let impossible = Board::from_fen("7k/8/8/3p4/8/8/8/4K3 w - d6 0 1").unwrap();
    let no_ep = Board::from_fen("7k/8/8/3p4/8/8/8/4K3 w - - 0 1").unwrap();
    assert_eq!(book::key(&impossible), book::key(&no_ep));
}
