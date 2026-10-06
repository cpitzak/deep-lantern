//! Iterative-deepening PVS, bounded TT, quiescence, LMR, null-move pruning and lazy SMP.
use crate::{
    board::*,
    eval::{evaluate, VALUES},
};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    time::{Duration, Instant},
};
pub const MATE: i32 = 30000;
const INF: i32 = 32000;
const MAX_PLY: usize = 128;
#[derive(Copy, Clone)]
struct Entry {
    key: u64,
    mv: Move,
    score: i32,
    depth: i32,
    bound: u8,
    age: u8,
    clock: u8,
}
struct Slot {
    version: AtomicU64,
    key: AtomicU64,
    data: AtomicU64,
}
/// Nonblocking seqlock slots. Sequential consistency prevents torn key/data observations.
pub struct Table {
    slots: Vec<Slot>,
    generation: std::sync::atomic::AtomicU8,
}
impl Table {
    pub fn new(mb: usize) -> Self {
        let count = (mb.clamp(1, 4096) * 1024 * 1024 / std::mem::size_of::<Slot>()).max(1);
        Self {
            slots: (0..count)
                .map(|_| Slot {
                    version: AtomicU64::new(0),
                    key: AtomicU64::new(0),
                    data: AtomicU64::new(0),
                })
                .collect(),
            generation: std::sync::atomic::AtomicU8::new(0),
        }
    }
    pub fn clear(&self) {
        for s in &self.slots {
            s.data.store(0, Ordering::SeqCst);
            s.key.store(0, Ordering::SeqCst);
        }
    }
    pub fn new_search(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
    fn get(&self, key: u64) -> Option<Entry> {
        let s = &self.slots[key as usize % self.slots.len()];
        let v = s.version.load(Ordering::SeqCst);
        if v & 1 != 0 {
            return None;
        }
        let k = s.key.load(Ordering::SeqCst);
        let d = s.data.load(Ordering::SeqCst);
        if v != s.version.load(Ordering::SeqCst) || k != key || d == 0 {
            return None;
        }
        Some(Entry {
            key: k,
            mv: Move(d as u16),
            score: ((d >> 16) as u16 as i16) as i32,
            depth: ((d >> 32) & 255) as i32,
            bound: ((d >> 40) & 3) as u8,
            age: ((d >> 42) & 255) as u8,
            clock: ((d >> 50) & 127) as u8,
        })
    }
    fn put(&self, mut e: Entry) {
        e.age = self.generation.load(Ordering::Relaxed);
        if let Some(old) = self.get(e.key) {
            if old.age == e.age && old.depth > e.depth + 3 && e.bound != 0 {
                return;
            }
        }
        let s = &self.slots[e.key as usize % self.slots.len()];
        let v = s.version.load(Ordering::SeqCst);
        if v & 1 != 0
            || s.version
                .compare_exchange(v, v.wrapping_add(1), Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            return;
        }
        let d = e.mv.0 as u64
            | ((e.score as i16 as u16 as u64) << 16)
            | ((e.depth.clamp(0, 255) as u64) << 32)
            | ((e.bound as u64) << 40)
            | ((e.age as u64) << 42)
            | ((e.clock as u64) << 50)
            | (1 << 63);
        s.key.store(e.key, Ordering::SeqCst);
        s.data.store(d, Ordering::SeqCst);
        s.version.store(v.wrapping_add(2), Ordering::SeqCst);
    }
}
/// Per-search clock and output gate. Pondering does not spend the engine's clock.
/// The UCI thread releases the gate; search completion alone never releases it.
pub struct Control {
    origin: Instant,
    clock_start_us: AtomicU64,
    released: Mutex<bool>,
    wake: Condvar,
}
impl Control {
    pub fn new(ponder: bool, infinite: bool) -> Self {
        Self {
            origin: Instant::now(),
            clock_start_us: AtomicU64::new(if ponder { u64::MAX } else { 0 }),
            released: Mutex::new(!ponder && !infinite),
            wake: Condvar::new(),
        }
    }
    pub fn elapsed(&self) -> Option<Duration> {
        let start = self.clock_start_us.load(Ordering::Acquire);
        (start != u64::MAX).then(|| {
            self.origin
                .elapsed()
                .saturating_sub(Duration::from_micros(start))
        })
    }
    pub fn ponderhit(&self) {
        if self
            .clock_start_us
            .compare_exchange(
                u64::MAX,
                self.origin.elapsed().as_micros() as u64,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.release();
        }
    }
    pub fn release(&self) {
        *self.released.lock().unwrap() = true;
        self.wake.notify_all();
    }
    pub fn wait(&self) {
        let mut released = self.released.lock().unwrap();
        while !*released {
            released = self.wake.wait(released).unwrap();
        }
    }
}
#[derive(Clone)]
pub struct Limits {
    pub depth: u8,
    pub nodes: Option<u64>,
    pub soft: Option<Duration>,
    pub hard: Option<Duration>,
    pub root_moves: Option<Vec<Move>>,
    pub control: Option<Arc<Control>>,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: 64,
            nodes: None,
            soft: None,
            hard: None,
            root_moves: None,
            control: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Info {
    pub depth: u8,
    pub seldepth: usize,
    pub score: i32,
    pub nodes: u64,
    pub elapsed: Duration,
    pub pv: Vec<Move>,
}
#[derive(Debug)]
pub struct Outcome {
    pub best: Option<Move>,
    pub ponder: Option<Move>,
    pub score: i32,
    pub depth: u8,
    pub nodes: u64,
}
struct Shared {
    stop: Arc<AtomicBool>,
    nodes: AtomicU64,
    start: Instant,
    limits: Limits,
    table: Arc<Table>,
}
impl Shared {
    fn budget_elapsed(&self) -> Option<Duration> {
        match &self.limits.control {
            Some(control) => control.elapsed(),
            None => Some(self.start.elapsed()),
        }
    }
}
struct Searcher {
    shared: Arc<Shared>,
    killers: [[Move; 2]; MAX_PLY],
    history: [[[i32; 64]; 64]; 2],
    pv: [[Move; MAX_PLY]; MAX_PLY],
    pv_len: [usize; MAX_PLY],
    seldepth: usize,
    worker: usize,
}
impl Searcher {
    fn new(shared: Arc<Shared>, worker: usize) -> Self {
        Self {
            shared,
            killers: [[Move(0); 2]; MAX_PLY],
            history: [[[0; 64]; 64]; 2],
            pv: [[Move(0); MAX_PLY]; MAX_PLY],
            pv_len: [0; MAX_PLY],
            seldepth: 0,
            worker,
        }
    }
    fn stopped(&self) -> bool {
        self.shared.stop.load(Ordering::Relaxed)
    }
    fn visit(&mut self, ply: usize) -> bool {
        self.seldepth = self.seldepth.max(ply);
        let n = self.shared.nodes.fetch_add(1, Ordering::Relaxed) + 1;
        if self.shared.limits.nodes.is_some_and(|cap| n >= cap)
            || (n & 255 == 0
                && self.shared.limits.hard.is_some_and(|t| {
                    self.shared
                        .budget_elapsed()
                        .is_some_and(|elapsed| elapsed >= t)
                }))
        {
            self.shared.stop.store(true, Ordering::Relaxed);
        }
        self.stopped()
    }
    fn move_score(&self, b: &Board, m: Move, tt: Move, ply: usize) -> i32 {
        if m == tt {
            return 2_000_000;
        }
        if b.is_promotion(m) {
            return 1_500_000 + VALUES[m.promo()];
        }
        if b.is_capture(m) {
            let victim = if m.flag() == EP {
                PAWN
            } else {
                b.squares[m.to()] as usize % 6
            };
            return 1_000_000 + VALUES[victim] * 16 - VALUES[b.squares[m.from()] as usize % 6];
        }
        if self.killers[ply][0] == m {
            return 900_000;
        }
        if self.killers[ply][1] == m {
            return 800_000;
        }
        self.history[b.side][m.from()][m.to()]
    }
    fn ordered(&self, b: &Board, tt: Move, ply: usize) -> MoveList {
        let mut list = b.legal_moves();
        list.moves[..list.len]
            .sort_unstable_by_key(|&m| std::cmp::Reverse(self.move_score(b, m, tt, ply)));
        if ply == 0 && self.worker > 0 && list.len > 1 {
            let shift = self.worker % list.len;
            list.moves[..list.len].rotate_left(shift);
        }
        list
    }
    fn draw(b: &Board, null_path: bool) -> bool {
        b.insufficient() || (!null_path && (b.halfmove >= 100 || b.repetition()))
    }
    fn qsearch(
        &mut self,
        b: &mut Board,
        mut alpha: i32,
        beta: i32,
        ply: usize,
        null_path: bool,
    ) -> i32 {
        self.pv_len[ply] = ply;
        if self.visit(ply) {
            return 0;
        }
        let check = b.in_check();
        let moves = self.ordered(b, Move(0), ply);
        if moves.len == 0 {
            return if check { -MATE + ply as i32 } else { 0 };
        }
        if Self::draw(b, null_path) {
            return 0;
        }
        if ply >= MAX_PLY - 2 {
            return evaluate(b);
        }
        if !check {
            let stand = evaluate(b);
            if stand >= beta {
                return stand;
            }
            alpha = alpha.max(stand);
        }
        for &m in moves.as_slice() {
            if !check && !b.is_capture(m) && !b.is_promotion(m) {
                continue;
            }
            let u = b.make(m);
            let value = -self.qsearch(b, -beta, -alpha, ply + 1, null_path);
            b.unmake(m, u);
            if self.stopped() {
                return 0;
            }
            if value >= beta {
                return value;
            }
            if value > alpha {
                alpha = value;
                self.update_pv(ply, m);
            }
        }
        alpha
    }
    fn update_pv(&mut self, ply: usize, m: Move) {
        self.pv[ply][ply] = m;
        let len = self.pv_len[ply + 1].max(ply + 1);
        for i in ply + 1..len {
            self.pv[ply][i] = self.pv[ply + 1][i];
        }
        self.pv_len[ply] = len;
    }
    #[allow(clippy::too_many_arguments)]
    fn search(
        &mut self,
        b: &mut Board,
        mut depth: i32,
        mut alpha: i32,
        mut beta: i32,
        ply: usize,
        allow_null: bool,
        null_path: bool,
    ) -> i32 {
        self.pv_len[ply] = ply;
        if depth <= 0 {
            return self.qsearch(b, alpha, beta, ply, null_path);
        }
        if self.visit(ply) {
            return 0;
        }
        let check = b.in_check();
        if ply >= MAX_PLY - 2 {
            return evaluate(b);
        }
        // Checkmate takes precedence over a fifty-move claim.
        if ply > 0 && Self::draw(b, null_path) {
            return if check && b.legal_moves().len == 0 {
                -MATE + ply as i32
            } else {
                0
            };
        }
        if ply > 0 {
            alpha = alpha.max(-MATE + ply as i32);
            beta = beta.min(MATE - ply as i32 - 1);
            if alpha >= beta {
                return alpha;
            }
        }
        if check {
            depth += 1;
        }
        let original = alpha;
        let pv_node = beta - alpha > 1;
        let entry = self.shared.table.get(b.key);
        let tt = entry.map_or(Move(0), |e| e.mv);
        // Repetition-sensitive paths must not reuse or store history-independent bounds.
        let repetition_sensitive = b
            .history
            .iter()
            .rev()
            .skip(1)
            .take(b.halfmove as usize)
            .any(|&k| k == b.key);
        let use_bounds = !null_path && !repetition_sensitive;
        if let Some(e) = entry {
            let score = from_tt(e.score, ply);
            if use_bounds
                && ply > 0
                && !pv_node
                && e.clock == b.halfmove.min(127) as u8
                && e.depth >= depth
                && (e.bound == 0 || e.bound == 1 && score >= beta || e.bound == 2 && score <= alpha)
            {
                return score;
            }
        }
        let static_eval = evaluate(b);
        if allow_null
            && !null_path
            && !pv_node
            && !check
            && ply > 0
            && depth >= 3
            && static_eval >= beta
            && b.non_pawn_material()
            && beta.abs() < MATE - 1000
        {
            let u = b.make_null();
            let score = -self.search(
                b,
                depth - 1 - (2 + depth / 4),
                -beta,
                -beta + 1,
                ply + 1,
                false,
                true,
            );
            b.unmake_null(u);
            if self.stopped() {
                return 0;
            }
            if score >= beta {
                return beta;
            }
        }
        let mut moves = self.ordered(b, tt, ply);
        if ply == 0 {
            if let Some(ref allowed) = self.shared.limits.root_moves {
                let mut filtered = MoveList::default();
                for &m in moves.as_slice() {
                    if allowed.contains(&m) {
                        filtered.push(m);
                    }
                }
                moves = filtered;
            }
        }
        if moves.len == 0 {
            return if check { -MATE + ply as i32 } else { 0 };
        }
        let mut best = -INF;
        let mut best_move = moves.moves[0];
        let mut quiets = Vec::with_capacity(32);
        for (index, &m) in moves.as_slice().iter().enumerate() {
            let quiet = !b.is_capture(m) && !b.is_promotion(m);
            let u = b.make(m);
            let gives_check = b.in_check();
            let next = depth - 1;
            let mut score;
            if index == 0 {
                score = -self.search(b, next, -beta, -alpha, ply + 1, true, null_path);
            } else {
                let reduction = if depth >= 3 && index >= 4 && quiet && !check && !gives_check {
                    let base = 1 + ((depth as f64).ln() * (index as f64).ln() / 2.5) as i32;
                    (base - if pv_node { 1 } else { 0 }).clamp(1, (next - 1).max(1))
                } else {
                    0
                };
                score = -self.search(
                    b,
                    next - reduction,
                    -alpha - 1,
                    -alpha,
                    ply + 1,
                    true,
                    null_path,
                );
                if score > alpha && reduction > 0 {
                    score = -self.search(b, next, -alpha - 1, -alpha, ply + 1, true, null_path);
                }
                if score > alpha && score < beta {
                    score = -self.search(b, next, -beta, -alpha, ply + 1, true, null_path);
                }
            }
            b.unmake(m, u);
            if self.stopped() {
                return 0;
            }
            if score > best {
                best = score;
                best_move = m;
            }
            if score > alpha {
                alpha = score;
                self.update_pv(ply, m);
            }
            if alpha >= beta {
                if quiet {
                    if self.killers[ply][0] != m {
                        self.killers[ply][1] = self.killers[ply][0];
                        self.killers[ply][0] = m;
                    }
                    let bonus = (depth * depth * 16).min(2000);
                    Self::adjust(&mut self.history[b.side][m.from()][m.to()], bonus);
                    for q in quiets {
                        let q: Move = q;
                        Self::adjust(&mut self.history[b.side][q.from()][q.to()], -bonus);
                    }
                }
                break;
            }
            if quiet {
                quiets.push(m);
            }
        }
        if use_bounds {
            self.shared.table.put(Entry {
                key: b.key,
                mv: best_move,
                score: to_tt(best, ply),
                depth,
                bound: if best >= beta {
                    1
                } else if best <= original {
                    2
                } else {
                    0
                },
                age: 0,
                clock: b.halfmove.min(127) as u8,
            });
        }
        best
    }
    fn adjust(slot: &mut i32, bonus: i32) {
        *slot += bonus - *slot * bonus.abs() / 16384;
    }
    fn iterate(&mut self, b: &mut Board, report: &mut dyn FnMut(Info)) -> Outcome {
        let moves = b.legal_moves();
        let mut best = moves.as_slice().iter().copied().find(|m| {
            self.shared
                .limits
                .root_moves
                .as_ref()
                .is_none_or(|v| v.contains(m))
        });
        let mut ponder = None;
        let mut score = 0;
        let mut completed = 0;
        if best.is_none() {
            return Outcome {
                best: None,
                ponder: None,
                score: if b.in_check() { -MATE } else { 0 },
                depth: 0,
                nodes: 0,
            };
        }
        for depth in 1..=self.shared.limits.depth.min(96) {
            let mut window = if depth >= 5 { 25 } else { INF };
            let mut alpha = if window == INF { -INF } else { score - window };
            let mut beta = if window == INF { INF } else { score + window };
            let value = loop {
                let value = self.search(b, depth as i32, alpha, beta, 0, true, false);
                if self.stopped() {
                    break value;
                }
                if value <= alpha {
                    alpha = (value - window).max(-INF);
                } else if value >= beta {
                    beta = (value + window).min(INF);
                } else {
                    break value;
                }
                window = (window * 2).min(INF);
            };
            if self.stopped() {
                break;
            }
            score = value;
            completed = depth;
            if self.pv_len[0] > 0 {
                best = Some(self.pv[0][0]);
                ponder = (self.pv_len[0] > 1).then(|| self.pv[0][1]);
            }
            if self.worker == 0 {
                report(Info {
                    depth,
                    seldepth: self.seldepth,
                    score,
                    nodes: self.shared.nodes.load(Ordering::Relaxed),
                    elapsed: self.shared.start.elapsed(),
                    pv: self.pv[0][..self.pv_len[0]].to_vec(),
                });
                if self.shared.limits.soft.is_some_and(|t| {
                    self.shared
                        .budget_elapsed()
                        .is_some_and(|elapsed| elapsed >= t)
                }) {
                    break;
                }
            }
        }
        Outcome {
            best,
            ponder,
            score,
            depth: completed,
            nodes: self.shared.nodes.load(Ordering::Relaxed),
        }
    }
}
fn to_tt(s: i32, ply: usize) -> i32 {
    if s > MATE - 1000 {
        s + ply as i32
    } else if s < -MATE + 1000 {
        s - ply as i32
    } else {
        s
    }
}
fn from_tt(s: i32, ply: usize) -> i32 {
    if s > MATE - 1000 {
        s - ply as i32
    } else if s < -MATE + 1000 {
        s + ply as i32
    } else {
        s
    }
}
pub fn run(
    board: Board,
    limits: Limits,
    table: Arc<Table>,
    stop: Arc<AtomicBool>,
    threads: usize,
    mut report: impl FnMut(Info),
) -> Outcome {
    table.new_search();
    let shared = Arc::new(Shared {
        stop,
        nodes: AtomicU64::new(0),
        start: Instant::now(),
        limits,
        table,
    });
    std::thread::scope(|scope| {
        for id in 1..threads.clamp(1, 64) {
            let shared = shared.clone();
            let mut board = board.clone();
            scope.spawn(move || {
                Searcher::new(shared, id).iterate(&mut board, &mut |_| {});
            });
        }
        let result = Searcher::new(shared.clone(), 0).iterate(&mut board.clone(), &mut report);
        shared.stop.store(true, Ordering::Relaxed);
        result
    })
}
