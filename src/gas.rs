//! Gaz: losowo rozmieszczone cząsteczki krążące wokół gwiazdy w cienkim dysku.
//!
//! Cząsteczki czują grawitację gwiazdy i planet, same przyciągają planety (gdy gaz ma masę),
//! a w obrębie sfery Hilla planety wymieniają z nią pęd — działa to jak opór gazu.
//! Cząsteczki nie oddziałują między sobą.

use std::f64::consts::TAU;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::physics::{Body, G, Simulation, kepler_to_state};
use crate::profile::DensityProfile;
use crate::system::{EARTH_MASS_IN_SUNS, SystemConfig};

#[derive(Clone, Debug)]
pub struct GasConfig {
    pub enabled: bool,
    pub count: usize,
    /// Całkowita masa gazu [M⊕].
    pub total_mass_earth: f64,
    /// Promień wewnętrzny dysku [AU].
    pub inner: f64,
    /// Promień zewnętrzny dysku [AU].
    pub outer: f64,
    /// Największe nachylenie orbit cząsteczek [°].
    pub thickness: f64,
    /// Siła oporu: 0 = tylko grawitacja, 1 = hamowanie w ciągu ~1/2π obiegu.
    pub drag: f64,
    /// Początkowy rozkład gęstości powierzchniowej w funkcji promienia.
    pub profile: DensityProfile,
}

impl GasConfig {
    pub fn for_system(cfg: &SystemConfig) -> Self {
        let mut gas = Self {
            enabled: false,
            count: 300,
            total_mass_earth: 10.0,
            inner: 0.0,
            outer: 0.0,
            thickness: 3.0,
            drag: 1.0,
            profile: DensityProfile::default(),
        };
        gas.fit_to(cfg);
        gas
    }

    /// Dopasowuje rozmiar dysku do orbit planet układu.
    pub fn fit_to(&mut self, cfg: &SystemConfig) {
        let a_min = cfg.planets.iter().map(|p| p.a).fold(f64::INFINITY, f64::min);
        let a_max = cfg.planets.iter().map(|p| p.a).fold(0.0, f64::max);
        let (a_min, a_max) = if a_min.is_finite() { (a_min, a_max) } else { (1.0, 1.0) };
        self.inner = a_min * 0.5;
        self.outer = a_max * 1.3;
    }

    /// Masa jednej cząsteczki [M☉], gdy gaz dzieli się na `n` cząsteczek.
    pub fn particle_mass(&self, n: usize) -> f64 {
        self.total_mass_earth.max(0.0) * EARTH_MASS_IN_SUNS / n.max(1) as f64
    }

    /// Cząsteczki zbliżone do gwiazdy bardziej niż to lub uciekające dalej niż to są usuwane [AU].
    pub fn removal_bounds(&self) -> (f64, f64) {
        let (lo, hi) = self.radii();
        (lo * 0.3, hi * 5.0)
    }

    /// Promień wewnętrzny i zewnętrzny dysku [AU], w kolejności rosnącej.
    pub fn radii(&self) -> (f64, f64) {
        (self.inner.min(self.outer), self.inner.max(self.outer))
    }

    /// Losuje cząsteczki na orbitach prawie kołowych wokół gwiazdy, z gęstością powierzchniową
    /// zgodną z `profile`. Błąd, gdy profilu nie da się policzyć (np. błędny wzór).
    pub fn scatter(&self, sim: &Simulation) -> Result<Vec<Body>, String> {
        let mut rng = Rng::from_time();
        let star = &sim.bodies[0];
        let mu = G * star.mass;
        let mass = self.particle_mass(self.count);
        let (lo, hi) = self.radii();
        let radius = self.profile.radius_sampler(lo, hi)?;
        let max_inc = self.thickness.to_radians();
        Ok((0..self.count)
            .map(|_| {
                let (pos, vel) = kepler_to_state(
                    mu,
                    radius.radius(rng.next()),
                    0.02 * rng.next(),
                    (rng.next() * 2.0 - 1.0) * max_inc,
                    TAU * rng.next(),
                    TAU * rng.next(),
                    TAU * rng.next(),
                );
                Body::new(star.pos + pos, star.vel + vel, mass)
            })
            .collect())
    }
}

/// Generator xorshift64 — wystarczający do rozsiewania cząsteczek.
struct Rng(u64);

impl Rng {
    fn from_time() -> Self {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        Self(nanos | 1)
    }

    /// Liczba z przedziału [0, 1).
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Preset;

    #[test]
    fn gas_stays_finite_in_all_presets() {
        for preset in Preset::ALL {
            let cfg = preset.config();
            let mut gas = GasConfig::for_system(&cfg);
            gas.count = 200;
            gas.total_mass_earth = 1000.0;
            let mut sim = Simulation::new(&cfg);
            sim.set_particles(gas.scatter(&sim).unwrap(), gas.removal_bounds(), gas.drag);
            sim.advance(cfg.speed * 10.0, 2_000_000);
            assert!(
                sim.bodies.iter().chain(&sim.particles).all(|b| b.pos.length().is_finite()),
                "{}",
                preset.label()
            );
            assert!(sim.particles.len() > 100, "{}: zostało {} cząsteczek", preset.label(), sim.particles.len());
        }
    }
}
