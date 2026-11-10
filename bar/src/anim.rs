// Animation primitives. Everything animates on the bar's own clock (seconds since start) and only while something
// is moving; when nothing is, the bar stops asking for frames.

/// A spring toward a target. The default is the "medium" spring picked in Bar Lab: a small overshoot (~8%).
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    stiffness: f32,
    damping: f32,
}

impl Spring {
    pub fn new(v: f32) -> Self {
        // Damping ratio 24 / (2 * sqrt(380)) ≈ 0.62.
        Self { value: v, velocity: 0.0, target: v, stiffness: 380.0, damping: 24.0 }
    }

    /// Without the overshoot: for sizes and positions that must not bounce past their final value.
    pub fn smooth(v: f32) -> Self {
        Self { stiffness: 300.0, damping: 2.0 * 300f32.sqrt(), ..Self::new(v) }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
    }

    pub fn snap(&mut self, v: f32) {
        (self.value, self.velocity, self.target) = (v, 0.0, v);
    }

    pub fn moving(&self) -> bool {
        (self.value - self.target).abs() > 1e-3 || self.velocity.abs() > 1e-2
    }

    /// Advances by `dt` seconds (sub-stepped for stability) and settles exactly on the target at the end.
    pub fn step(&mut self, dt: f32) {
        if !self.moving() {
            self.snap(self.target);
            return;
        }
        let n = (dt / (1.0 / 480.0)).ceil().clamp(1.0, 64.0);
        let h = dt / n;
        for _ in 0..n as usize {
            let a = -self.stiffness * (self.value - self.target) - self.damping * self.velocity;
            self.velocity += a * h;
            self.value += self.velocity * h;
        }
        if !self.moving() {
            self.snap(self.target);
        }
    }
}

/// A timed ease-out transition between two values.
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    start: f32,
    dur: f32,
}

impl Tween {
    pub fn new(v: f32) -> Self {
        Self { from: v, to: v, start: 0.0, dur: 0.0 }
    }

    /// Starts from wherever the current transition is now, so retargeting mid-way never jumps.
    pub fn to(&mut self, target: f32, now: f32, dur: f32) {
        if target != self.to {
            (self.from, self.to, self.start, self.dur) = (self.get(now), target, now, dur);
        }
    }

    pub fn snap(&mut self, v: f32) {
        (self.from, self.to, self.dur) = (v, v, 0.0);
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn get(&self, now: f32) -> f32 {
        let t = if self.dur <= 0.0 { 1.0 } else { ((now - self.start) / self.dur).clamp(0.0, 1.0) };
        let e = 1.0 - (1.0 - t).powi(3); // ease-out cubic
        self.from + (self.to - self.from) * e
    }

    pub fn moving(&self, now: f32) -> bool {
        self.dur > 0.0 && now < self.start + self.dur
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_overshoots_a_little_then_settles() {
        let mut s = Spring::new(0.0);
        s.set(1.0);
        let mut peak: f32 = 0.0;
        for _ in 0..144 {
            s.step(1.0 / 144.0);
            peak = peak.max(s.value);
        }
        assert!(peak > 1.03 && peak < 1.15, "peak {peak}");
        assert!(!s.moving() && s.value == 1.0);
    }

    #[test]
    fn smooth_spring_never_overshoots() {
        let mut s = Spring::smooth(0.0);
        s.set(10.0);
        for _ in 0..300 {
            s.step(1.0 / 144.0);
            assert!(s.value <= 10.0 + 1e-3);
        }
        assert_eq!(s.value, 10.0);
    }

    #[test]
    fn tween_eases_and_retargets_without_jumping() {
        let mut t = Tween::new(0.0);
        t.to(100.0, 0.0, 1.0);
        assert!(t.get(0.5) > 50.0); // ease-out is ahead of linear
        let mid = t.get(0.5);
        t.to(0.0, 0.5, 1.0);
        assert_eq!(t.get(0.5), mid);
        assert!(!t.moving(1.6) && t.get(1.6) == 0.0);
    }
}
