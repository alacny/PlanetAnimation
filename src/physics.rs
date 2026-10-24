//! Symulacja grawitacyjna N ciał (całkowanie metodą leapfrog KDK z adaptacyjnym krokiem).
//!
//! Jednostki: AU, lata, masy Słońca — wtedy G = 4π².

use std::collections::VecDeque;
use std::f64::consts::TAU;

use rayon::prelude::*;

use crate::system::{EARTH_MASS_IN_SUNS, SystemConfig};
use crate::vec3::Vec3;

pub const G: f64 = TAU * TAU;

/// Liczba punktów śladu przypadająca na jeden obieg planety.
pub const TRAIL_SAMPLES_PER_ORBIT: f64 = 180.0;

/// Ułamek najkrótszej skali czasowej układu użyty jako krok całkowania.
const ETA: f64 = 0.004;

/// Od tylu cząsteczek obliczenia dzielą się między wątki; poniżej narzut synchronizacji
/// przewyższa zysk.
const PARALLEL_THRESHOLD: usize = 64;

/// Tyle kroków ciał liczy się z góry, zanim cząsteczki je dogonią; wpływ cząsteczek na ciała
/// wraca do nich jako popęd raz na porcję (opóźnienie ≈ 1/25 obiegu najszybszego ciała).
const BATCH_STEPS: usize = 64;

#[derive(Clone, Debug)]
pub struct Body {
    pub pos: Vec3,
    pub vel: Vec3,
    /// Masa w masach Słońca.
    pub mass: f64,
    acc: Vec3,
}

impl Body {
    pub fn new(pos: Vec3, vel: Vec3, mass: f64) -> Self {
        Self { pos, vel, mass, acc: Vec3::ZERO }
    }
}

/// Oskulacyjne elementy orbitalne względem gwiazdy (kąty w stopniach).
#[derive(Clone, Copy, Debug)]
pub struct OrbitalElements {
    /// Wielka półoś [AU] (ujemna dla orbity hiperbolicznej).
    pub a: f64,
    pub e: f64,
    pub inclination: f64,
    pub node: f64,
    pub periapsis: f64,
    pub anomaly: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Trail {
    /// Pozycje względem gwiazdy centralnej.
    pub points: VecDeque<Vec3>,
    last_sample: f64,
}

#[derive(Clone)]
pub struct Simulation {
    /// Indeks 0 to gwiazda centralna, dalej planety w kolejności z konfiguracji.
    pub bodies: Vec<Body>,
    pub trails: Vec<Trail>,
    /// Czas symulacji [lata].
    pub time: f64,
    /// Maksymalna liczba punktów śladu.
    pub trail_capacity: usize,
    /// Cząsteczki gazu (nie oddziałują między sobą).
    pub particles: Vec<Body>,
    /// Liczba cząsteczek pochłoniętych przez gwiazdę lub wyrzuconych z układu.
    pub particles_lost: usize,
    /// Odległości od gwiazdy, poza którymi cząsteczka jest usuwana [AU].
    particle_bounds: (f64, f64),
    /// Siła oporu gazu: odwrotność czasu hamowania w jednostkach lokalnej skali orbitalnej.
    gas_drag: f64,
    /// Promienie sfer Hilla ciał z ostatniego obliczenia przyspieszeń (0 dla gwiazdy).
    hill: Vec<f64>,
    softening_sq: f64,
    reference_energy: f64,
}

impl Simulation {
    pub fn new(cfg: &SystemConfig) -> Self {
        let star_mass = cfg.star_mass.max(1e-6);
        let mut bodies = vec![Body::new(Vec3::ZERO, Vec3::ZERO, star_mass)];

        for p in &cfg.planets {
            let mass = p.mass_earth.max(0.0) * EARTH_MASS_IN_SUNS;
            let (pos, vel) = kepler_to_state(
                G * (star_mass + mass),
                p.a.max(1e-5),
                p.e.clamp(0.0, 0.99),
                p.inclination.to_radians(),
                p.node.to_radians(),
                p.periapsis.to_radians(),
                p.anomaly.to_radians(),
            );
            bodies.push(Body::new(pos, vel, mass));
        }

        // Przejście do układu środka masy, by cały układ nie dryfował.
        let total: f64 = bodies.iter().map(|b| b.mass).sum();
        let com = bodies.iter().fold(Vec3::ZERO, |s, b| s + b.pos * b.mass) / total;
        let vcom = bodies.iter().fold(Vec3::ZERO, |s, b| s + b.vel * b.mass) / total;
        for b in &mut bodies {
            b.pos -= com;
            b.vel -= vcom;
        }

        let scale = cfg.planets.iter().map(|p| p.a).fold(0.0, f64::max).max(0.01);
        let mut sim = Self {
            trails: vec![Trail::default(); bodies.len()],
            bodies,
            time: 0.0,
            trail_capacity: TRAIL_SAMPLES_PER_ORBIT as usize,
            particles: Vec::new(),
            particles_lost: 0,
            particle_bounds: (0.0, f64::INFINITY),
            gas_drag: 0.0,
            hill: Vec::new(),
            softening_sq: (scale * 1e-5).powi(2),
            reference_energy: 0.0,
        };
        sim.compute_body_accelerations();
        sim.reference_energy = sim.total_energy();
        sim
    }

    /// Zmienia masę ciała w trakcie symulacji (pozycje i prędkości pozostają bez zmian).
    pub fn set_mass(&mut self, index: usize, mass_suns: f64) {
        if let Some(b) = self.bodies.get_mut(index) {
            b.mass = mass_suns.max(if index == 0 { 1e-6 } else { 0.0 });
            self.compute_body_accelerations();
            self.reference_energy = self.total_energy();
        }
    }

    /// Zastępuje cząsteczki gazu nowymi; `bounds` to odległości od gwiazdy, poza którymi są usuwane.
    pub fn set_particles(&mut self, particles: Vec<Body>, bounds: (f64, f64), drag: f64) {
        self.particles = particles;
        self.particles_lost = 0;
        self.particle_bounds = bounds;
        self.gas_drag = drag.max(0.0);
        self.compute_body_accelerations();
        self.reference_energy = self.total_energy();
    }

    /// Zmienia masę pojedynczej cząsteczki [M☉] i siłę oporu gazu bez ponownego rozsiewania.
    pub fn set_gas_coupling(&mut self, particle_mass: f64, drag: f64) {
        for p in &mut self.particles {
            p.mass = particle_mass.max(0.0);
        }
        self.gas_drag = drag.max(0.0);
        self.compute_body_accelerations();
        self.reference_energy = self.total_energy();
    }

    /// Przesuwa symulację o `duration` lat, wykonując co najwyżej `max_steps` kroków.
    /// Zwraca liczbę wykonanych kroków.
    pub fn advance(&mut self, duration: f64, max_steps: usize) -> usize {
        let end = self.time + duration;
        let mut steps = 0;
        let mut frames = Vec::new();
        while self.time < end && steps < max_steps {
            frames.clear();
            while self.time < end && steps < max_steps && frames.len() < BATCH_STEPS {
                let dt = (ETA * self.min_timescale()).min(end - self.time).max(1e-12);
                let before = (!self.particles.is_empty()).then(|| self.bodies.clone());
                self.step_bodies(dt);
                steps += 1;
                if let Some(mut mid) = before {
                    // Cząsteczki widzą ciała w połowie kroku.
                    for (m, b) in mid.iter_mut().zip(&self.bodies) {
                        m.pos = (m.pos + b.pos) * 0.5;
                        m.vel = (m.vel + b.vel) * 0.5;
                    }
                    frames.push(BodyFrame { dt, bodies: mid, hill: self.hill.clone() });
                }
            }
            self.advance_particles(&frames);
        }
        steps
    }

    /// Krok leapfrog samych ciał (cząsteczki dołączają w `advance_particles`).
    fn step_bodies(&mut self, dt: f64) {
        for b in &mut self.bodies {
            b.vel += b.acc * (0.5 * dt);
            b.pos += b.vel * dt;
        }
        self.compute_body_accelerations();
        for b in &mut self.bodies {
            b.vel += b.acc * (0.5 * dt);
        }
        self.time += dt;
        self.record_trails();
    }

    /// Prowadzi cząsteczki przez zapisane kroki ciał — porcjami cząsteczek na osobnych wątkach —
    /// i oddaje ciałom zebrany popęd od grawitacji gazu i oporu. Cząsteczki używają schematu
    /// drift–kick–drift, więc każde pchnięcie ma dokładnie przeciwny odpowiednik w popędzie ciał.
    fn advance_particles(&mut self, frames: &[BodyFrame]) {
        if frames.is_empty() {
            return;
        }
        let fields: Vec<ParticleField> = frames
            .iter()
            .map(|f| ParticleField::new(&f.bodies, &f.hill, self.softening_sq, self.gas_drag))
            .collect();
        let impulse = for_each_particle(&mut self.particles, self.bodies.len(), |p, impulse| {
            for (frame, field) in frames.iter().zip(&fields) {
                let dt = frame.dt;
                p.pos += p.vel * (0.5 * dt);
                field.accelerate(p, impulse, dt);
                p.vel += p.acc * dt;
                field.drag(p, dt, impulse);
                p.pos += p.vel * (0.5 * dt);
            }
        });
        for (b, dv) in self.bodies.iter_mut().zip(impulse) {
            b.vel += dv;
        }
        self.remove_lost_particles();
    }

    /// Przyspieszenia od wzajemnego przyciągania ciał oraz promienie ich sfer Hilla.
    fn compute_body_accelerations(&mut self) {
        let n = self.bodies.len();
        for b in &mut self.bodies {
            b.acc = Vec3::ZERO;
        }
        for i in 0..n {
            for j in (i + 1)..n {
                let d = self.bodies[j].pos - self.bodies[i].pos;
                let r2 = d.length_sq() + self.softening_sq;
                let f = d * (G / (r2 * r2.sqrt()));
                let (mi, mj) = (self.bodies[i].mass, self.bodies[j].mass);
                self.bodies[i].acc += f * mj;
                self.bodies[j].acc -= f * mi;
            }
        }

        let (star_pos, star_mass) = (self.bodies[0].pos, self.bodies[0].mass);
        self.hill = self
            .bodies
            .iter()
            .map(|b| (b.pos - star_pos).length() * (b.mass / (3.0 * star_mass)).cbrt())
            .collect();
    }

    fn remove_lost_particles(&mut self) {
        let star = self.bodies[0].pos;
        let (inner, outer) = self.particle_bounds;
        let before = self.particles.len();
        self.particles.retain(|p| {
            let r2 = (p.pos - star).length_sq();
            r2 > inner * inner && r2 < outer * outer
        });
        self.particles_lost += before - self.particles.len();
    }

    /// Najkrótsza charakterystyczna skala czasu (≈ okres / 2π) spośród wszystkich par ciał.
    fn min_timescale(&self) -> f64 {
        let n = self.bodies.len();
        let mut best = f64::INFINITY;
        for i in 0..n {
            for j in (i + 1)..n {
                let mu = G * (self.bodies[i].mass + self.bodies[j].mass);
                if mu <= 0.0 {
                    continue;
                }
                let r2 = (self.bodies[j].pos - self.bodies[i].pos).length_sq() + self.softening_sq;
                best = best.min((r2 * r2.sqrt() / mu).sqrt());
            }
        }
        if best.is_finite() { best } else { 1.0 }
    }

    fn record_trails(&mut self) {
        let star = self.bodies[0].clone();
        for (b, trail) in self.bodies.iter().zip(self.trails.iter_mut()).skip(1) {
            let rel = b.pos - star.pos;
            let r = rel.length();
            let period = TAU * (r * r * r / (G * (star.mass + b.mass))).sqrt();
            if self.time - trail.last_sample >= period / TRAIL_SAMPLES_PER_ORBIT || trail.points.is_empty() {
                trail.last_sample = self.time;
                trail.points.push_back(rel);
            }
            while trail.points.len() > self.trail_capacity {
                trail.points.pop_front();
            }
        }
    }

    pub fn total_energy(&self) -> f64 {
        let mut e = 0.0;
        for (i, a) in self.bodies.iter().enumerate() {
            e += 0.5 * a.mass * a.vel.length_sq();
            for b in &self.bodies[i + 1..] {
                e -= G * a.mass * b.mass / ((b.pos - a.pos).length_sq() + self.softening_sq).sqrt();
            }
        }
        e
    }

    /// Względna zmiana energii całkowitej od startu (miara dokładności całkowania).
    pub fn energy_drift(&self) -> f64 {
        if self.reference_energy == 0.0 {
            return 0.0;
        }
        ((self.total_energy() - self.reference_energy) / self.reference_energy).abs()
    }

    /// Odległość ciała od gwiazdy [AU] i jego prędkość względem niej [AU/rok].
    pub fn relative_to_star(&self, index: usize) -> (f64, f64) {
        let (s, b) = (&self.bodies[0], &self.bodies[index]);
        ((b.pos - s.pos).length(), (b.vel - s.vel).length())
    }

    /// Oskulacyjne elementy orbitalne ciała względem gwiazdy.
    pub fn orbital_elements(&self, index: usize) -> OrbitalElements {
        let (s, b) = (&self.bodies[0], &self.bodies[index]);
        state_to_kepler(G * (s.mass + b.mass), b.pos - s.pos, b.vel - s.vel)
    }
}

/// Stan ciał w połowie jednego kroku, zapamiętany dla cząsteczek.
struct BodyFrame {
    dt: f64,
    bodies: Vec<Body>,
    hill: Vec<f64>,
}

/// Wykonuje `f` dla każdej cząsteczki — przy dużej liczbie cząsteczek równolegle,
/// po jednej porcji na wątek procesora — i sumuje zebrane zmiany prędkości ciał.
fn for_each_particle(particles: &mut [Body], n_bodies: usize, f: impl Fn(&mut Body, &mut [Vec3]) + Sync) -> Vec<Vec3> {
    let run = |chunk: &mut [Body]| {
        let mut impulse = vec![Vec3::ZERO; n_bodies];
        for p in chunk {
            f(p, &mut impulse);
        }
        impulse
    };
    if particles.len() < PARALLEL_THRESHOLD {
        return run(particles);
    }
    let chunk = particles.len().div_ceil(rayon::current_num_threads());
    particles
        .par_chunks_mut(chunk)
        .map(run)
        .reduce_with(|mut a, b| {
            for (a, b) in a.iter_mut().zip(b) {
                *a += b;
            }
            a
        })
        .unwrap_or_else(|| vec![Vec3::ZERO; n_bodies])
}

/// Stan ciał widziany przez cząsteczki w jednym kroku (tylko do odczytu, wspólny dla wątków).
struct ParticleField<'a> {
    bodies: &'a [Body],
    hill: &'a [f64],
    softening_sq: f64,
    gm_star: f64,
    drag: f64,
}

impl<'a> ParticleField<'a> {
    fn new(bodies: &'a [Body], hill: &'a [f64], softening_sq: f64, drag: f64) -> Self {
        Self { bodies, hill, softening_sq, gm_star: G * bodies[0].mass, drag }
    }

    /// Cząsteczka czuje wszystkie ciała i przyciąga je z masą własną — zmiana prędkości ciał
    /// w ciągu `dt` trafia do `impulse`. Grawitacja planety jest zmiękczona w obrębie jej
    /// sfery Hilla, by bliskie przeloty nie wymagały mikroskopijnych kroków.
    fn accelerate(&self, p: &mut Body, impulse: &mut [Vec3], dt: f64) {
        p.acc = Vec3::ZERO;
        for ((b, &h), impulse) in self.bodies.iter().zip(self.hill).zip(impulse) {
            let d = b.pos - p.pos;
            let r2 = d.length_sq() + 0.25 * h * h + self.softening_sq;
            let f = d * (G / (r2 * r2.sqrt()));
            p.acc += f * b.mass;
            *impulse -= f * (p.mass * dt);
        }
    }

    /// Opór gazu: cząsteczka w sferze Hilla planety wyrównuje z nią prędkość w czasie hamowania
    /// τ = (lokalna skala orbitalna) / siła oporu. Odrzut planety trafia do `impulse`;
    /// wymiana pędu jest zachowawcza.
    fn drag(&self, p: &mut Body, dt: f64, impulse: &mut [Vec3]) {
        if self.drag <= 0.0 {
            return;
        }
        for ((b, &h), impulse) in self.bodies.iter().zip(self.hill).zip(impulse).skip(1) {
            let total = p.mass + b.mass;
            if total <= 0.0 || (p.pos - b.pos).length_sq() >= h * h {
                continue;
            }
            let r = (p.pos - self.bodies[0].pos).length();
            let tau = (r * r * r / self.gm_star).sqrt() / self.drag;
            let dv = (p.vel - b.vel) * (1.0 - (-dt / tau).exp());
            p.vel -= dv * (b.mass / total);
            *impulse += dv * (p.mass / total);
        }
    }
}

/// Zamiana elementów orbitalnych na wektor stanu (położenie, prędkość) względem ciała centralnego.
pub fn kepler_to_state(mu: f64, a: f64, e: f64, inc: f64, node: f64, peri: f64, nu: f64) -> (Vec3, Vec3) {
    let p = a * (1.0 - e * e);
    let r = p / (1.0 + e * nu.cos());
    let pos_pf = Vec3::new(r * nu.cos(), r * nu.sin(), 0.0);
    let v = (mu / p).sqrt();
    let vel_pf = Vec3::new(-v * nu.sin(), v * (e + nu.cos()), 0.0);

    let rotate = |q: Vec3| {
        // Rz(Ω) · Rx(i) · Rz(ω)
        let (so, co) = peri.sin_cos();
        let q = Vec3::new(co * q.x - so * q.y, so * q.x + co * q.y, q.z);
        let (si, ci) = inc.sin_cos();
        let q = Vec3::new(q.x, ci * q.y - si * q.z, si * q.y + ci * q.z);
        let (sn, cn) = node.sin_cos();
        Vec3::new(cn * q.x - sn * q.y, sn * q.x + cn * q.y, q.z)
    };
    (rotate(pos_pf), rotate(vel_pf))
}

/// Zamiana wektora stanu względem ciała centralnego na elementy orbitalne.
/// Dla orbity w płaszczyźnie odniesienia przyjmuje Ω = 0, a dla kołowej ω = 0.
fn state_to_kepler(mu: f64, r: Vec3, v: Vec3) -> OrbitalElements {
    const EPS: f64 = 1e-10;
    let h = r.cross(v);
    let h_hat = h.normalized();
    let e_vec = v.cross(h) / mu - r.normalized();
    let e = e_vec.length();
    let a = 1.0 / (2.0 / r.length() - v.length_sq() / mu);

    let n = Vec3::Z.cross(h);
    let n_hat = if n.length() > EPS * h.length() { n.normalized() } else { Vec3::new(1.0, 0.0, 0.0) };
    let q_hat = h_hat.cross(n_hat);
    // Kąt w płaszczyźnie orbity mierzony od linii węzłów w kierunku ruchu.
    let angle = |w: Vec3| w.dot(q_hat).atan2(w.dot(n_hat));
    let (periapsis, anomaly) = if e > EPS {
        let w = angle(e_vec);
        (w, angle(r) - w)
    } else {
        (0.0, angle(r))
    };

    let deg = |x: f64| x.to_degrees().rem_euclid(360.0);
    OrbitalElements {
        a,
        e,
        inclination: h_hat.z.clamp(-1.0, 1.0).acos().to_degrees(),
        node: deg(n_hat.y.atan2(n_hat.x)),
        periapsis: deg(periapsis),
        anomaly: deg(anomaly),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Preset;

    #[test]
    fn earth_completes_one_orbit_per_year() {
        let mut cfg = Preset::SolarSystem.config();
        cfg.planets.retain(|p| p.name == "Ziemia");
        let mut sim = Simulation::new(&cfg);
        let start = sim.bodies[1].pos - sim.bodies[0].pos;
        sim.advance(1.0, usize::MAX);
        let end = sim.bodies[1].pos - sim.bodies[0].pos;
        assert!((end - start).length() < 0.01, "przesunięcie po roku: {}", (end - start).length());
    }

    #[test]
    fn solar_system_conserves_energy() {
        let mut sim = Simulation::new(&Preset::SolarSystem.config());
        sim.advance(20.0, usize::MAX);
        assert!(sim.energy_drift() < 1e-4, "dryf energii: {}", sim.energy_drift());
    }

    #[test]
    fn orbital_elements_round_trip() {
        let mu = G * 1.2;
        let (pos, vel) = kepler_to_state(
            mu,
            1.5,
            0.2,
            10f64.to_radians(),
            40f64.to_radians(),
            70f64.to_radians(),
            120f64.to_radians(),
        );
        let el = state_to_kepler(mu, pos, vel);
        let got = [el.a, el.e, el.inclination, el.node, el.periapsis, el.anomaly];
        let want = [1.5, 0.2, 10.0, 40.0, 70.0, 120.0];
        for (g, w) in got.iter().zip(want) {
            assert!((g - w).abs() < 1e-9, "{got:?} ≠ {want:?}");
        }
    }

    #[test]
    fn gas_drag_conserves_momentum() {
        let mut sim = Simulation::new(&Preset::SolarSystem.config());
        let jupiter = sim.bodies[5].clone();
        // Cząsteczka w sferze Hilla Jowisza, poruszająca się względem niego.
        let p = Body::new(jupiter.pos + Vec3::new(0.05, 0.0, 0.0), jupiter.vel + Vec3::new(0.0, 0.5, 0.0), 1e-4);
        sim.set_particles(vec![p], (0.0, f64::INFINITY), 5.0);
        let momentum = |s: &Simulation| s.bodies.iter().chain(&s.particles).fold(Vec3::ZERO, |m, b| m + b.vel * b.mass);
        let before = momentum(&sim);
        sim.advance(0.1, usize::MAX);
        assert!((momentum(&sim) - before).length() < 1e-12, "zmiana pędu: {:?}", momentum(&sim) - before);
    }

    #[test]
    fn all_presets_stay_finite() {
        for preset in Preset::ALL {
            let cfg = preset.config();
            let mut sim = Simulation::new(&cfg);
            sim.advance(cfg.speed * 30.0, 5_000_000);
            assert!(sim.bodies.iter().all(|b| b.pos.length().is_finite()), "{}", preset.label());
        }
    }
}
