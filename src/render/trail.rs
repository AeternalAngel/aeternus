use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailConfig {
    pub still_ms: u32,
    pub fast: f32,
    pub slow: f32,
    pub threshold: f32,
}

impl TrailConfig {
    pub fn enabled(&self) -> bool {
        self.still_ms > 0
    }
}

pub type Quad = [[f32; 2]; 4];

pub fn rect_quad(x: f32, y: f32, w: f32, h: f32) -> Quad {
    [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}

#[derive(Debug)]
pub struct Trail {
    corners: Quad,
    active: bool,
    cell: Option<(usize, usize)>,
    last_move: Option<Instant>,
    last_frame: Option<Instant>,
}

impl Default for Trail {
    fn default() -> Self {
        Self { corners: [[0.0; 2]; 4], active: false, cell: None, last_move: None, last_frame: None }
    }
}

fn len(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

impl Trail {
    pub fn reset(&mut self) {
        *self = Trail::default();
    }

    pub fn animating(&self) -> bool {
        self.active
    }

    pub fn quad(&self) -> Option<Quad> {
        self.active.then_some(self.corners)
    }

    pub fn update(
        &mut self,
        target: Option<(Quad, (usize, usize))>,
        cfg: &TrailConfig,
        cell: (f32, f32),
        now: Instant,
    ) {
        let mut dt = if self.active {
            self.last_frame.map(|t| now.saturating_duration_since(t).as_secs_f32()).unwrap_or(0.0).min(0.1)
        } else {
            0.0
        };
        self.last_frame = Some(now);
        let Some((quad, pos)) = target else {
            self.active = false;
            self.cell = None;
            return;
        };
        if !cfg.enabled() {
            self.corners = quad;
            self.active = false;
            self.cell = Some(pos);
            return;
        }
        if self.cell != Some(pos) {
            let still = self
                .last_move
                .map(|t| now.saturating_duration_since(t) >= Duration::from_millis(cfg.still_ms as u64))
                .unwrap_or(false);
            let far = match self.cell {
                Some((r, c)) => {
                    let dr = r as f32 - pos.0 as f32;
                    let dc = c as f32 - pos.1 as f32;
                    (dr * dr + dc * dc).sqrt() >= cfg.threshold
                }
                None => false,
            };
            if self.cell.is_some() && (self.active || (still && far)) {
                if !self.active {
                    dt = 0.0;
                }
                self.active = true;
            } else {
                self.corners = quad;
                self.active = false;
            }
            self.cell = Some(pos);
            self.last_move = Some(now);
        }
        if !self.active {
            self.corners = quad;
            return;
        }
        let center =
            [quad.iter().map(|p| p[0]).sum::<f32>() / 4.0, quad.iter().map(|p| p[1]).sum::<f32>() / 4.0];
        let eps = 0.5_f32.max(cell.0.min(cell.1) * 0.02);
        let mut done = true;
        for (corner, q) in self.corners.iter_mut().zip(quad.iter()) {
            let d = [q[0] - corner[0], q[1] - corner[1]];
            let dl = len(d);
            if dl < eps {
                *corner = *q;
                continue;
            }
            done = false;
            let diag = [q[0] - center[0], q[1] - center[1]];
            let gl = len(diag).max(1e-3);
            let dot = ((d[0] * diag[0] + d[1] * diag[1]) / (dl * gl)).clamp(-1.0, 1.0);
            let decay = cfg.fast + (cfg.slow - cfg.fast) * (1.0 - dot) * 0.5;
            let step = 1.0 - (-10.0 * dt / decay.max(1e-3)).exp2();
            corner[0] += d[0] * step;
            corner[1] += d[1] * step;
        }
        if done {
            self.corners = quad;
            self.active = false;
        }
    }
}

pub fn hull_triangles(q: &Quad) -> Vec<[f32; 2]> {
    let cx = q.iter().map(|p| p[0]).sum::<f32>() / 4.0;
    let cy = q.iter().map(|p| p[1]).sum::<f32>() / 4.0;
    let mut pts: Vec<[f32; 2]> = q.to_vec();
    pts.sort_by(|a, b| {
        let aa = (a[1] - cy).atan2(a[0] - cx);
        let bb = (b[1] - cy).atan2(b[0] - cx);
        aa.total_cmp(&bb)
    });
    let hull = pts;
    let n = hull.len();
    let mut out = Vec::with_capacity(n * 3);
    for i in 0..n {
        let a = hull[i];
        let b = hull[(i + 1) % n];
        out.push([cx, cy]);
        out.push(a);
        out.push(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> TrailConfig {
        TrailConfig { still_ms: 5, fast: 0.1, slow: 0.4, threshold: 2.0 }
    }

    #[test]
    fn disabled_never_animates() {
        let mut t = Trail::default();
        let c = TrailConfig { still_ms: 0, ..cfg() };
        let now = Instant::now();
        t.update(Some((rect_quad(0.0, 0.0, 10.0, 20.0), (0, 0))), &c, (10.0, 20.0), now);
        t.update(
            Some((rect_quad(100.0, 0.0, 10.0, 20.0), (0, 10))),
            &c,
            (10.0, 20.0),
            now + Duration::from_secs(1),
        );
        assert!(!t.animating());
    }

    #[test]
    fn long_jump_after_rest_animates_and_converges() {
        let mut t = Trail::default();
        let c = cfg();
        let mut now = Instant::now();
        t.update(Some((rect_quad(0.0, 0.0, 10.0, 20.0), (0, 0))), &c, (10.0, 20.0), now);
        now += Duration::from_millis(50);
        let target = rect_quad(100.0, 0.0, 10.0, 20.0);
        t.update(Some((target, (0, 10))), &c, (10.0, 20.0), now);
        assert!(t.animating());
        let q = t.quad().unwrap();
        assert!(q[0][0] < 100.0);
        for _ in 0..200 {
            now += Duration::from_millis(16);
            t.update(Some((target, (0, 10))), &c, (10.0, 20.0), now);
        }
        assert!(!t.animating());
    }

    #[test]
    fn leading_corners_move_faster() {
        let mut t = Trail::default();
        let c = cfg();
        let mut now = Instant::now();
        t.update(Some((rect_quad(0.0, 0.0, 10.0, 20.0), (0, 0))), &c, (10.0, 20.0), now);
        now += Duration::from_millis(50);
        let target = rect_quad(200.0, 0.0, 10.0, 20.0);
        t.update(Some((target, (0, 20))), &c, (10.0, 20.0), now);
        now += Duration::from_millis(16);
        t.update(Some((target, (0, 20))), &c, (10.0, 20.0), now);
        let q = t.quad().unwrap();
        assert!(q[1][0] - 10.0 > q[0][0]);
    }

    #[test]
    fn short_moves_do_not_animate() {
        let mut t = Trail::default();
        let c = cfg();
        let mut now = Instant::now();
        t.update(Some((rect_quad(0.0, 0.0, 10.0, 20.0), (0, 0))), &c, (10.0, 20.0), now);
        now += Duration::from_millis(50);
        t.update(Some((rect_quad(10.0, 0.0, 10.0, 20.0), (0, 1))), &c, (10.0, 20.0), now);
        assert!(!t.animating());
    }

    #[test]
    fn typing_quickly_does_not_animate() {
        let mut t = Trail::default();
        let c = TrailConfig { still_ms: 100, ..cfg() };
        let mut now = Instant::now();
        t.update(Some((rect_quad(0.0, 0.0, 10.0, 20.0), (0, 0))), &c, (10.0, 20.0), now);
        now += Duration::from_millis(10);
        t.update(Some((rect_quad(50.0, 0.0, 10.0, 20.0), (0, 5))), &c, (10.0, 20.0), now);
        assert!(!t.animating());
    }

    #[test]
    fn hull_is_a_fan() {
        let tris = hull_triangles(&rect_quad(0.0, 0.0, 10.0, 10.0));
        assert_eq!(tris.len(), 12);
    }
}
