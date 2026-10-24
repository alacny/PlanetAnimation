//! Wątek fizyki: symulacja liczy się w tle, niezależnie od rysowania interfejsu.
//!
//! Interfejs i wątek fizyki dzielą stan `Shared` chroniony muteksem. Wątek fizyki trzyma blokadę
//! tylko przez krótki wycinek obliczeń, a interfejs — tylko na czas skopiowania stanu do rysowania
//! albo wprowadzenia zmiany.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::logger::ElementLogger;
use crate::physics::Simulation;

/// Najdłuższy czas obliczeń pod jedną blokadą — dłużej interfejs czekałby na dostęp do stanu.
const SLICE: Duration = Duration::from_millis(4);
/// Limit kroków w jednym wycinku, by interfejs nie zamarzł przy ekstremalnych ustawieniach.
const MAX_STEPS_PER_SLICE: usize = 150_000;

pub struct Shared {
    pub sim: Simulation,
    pub running: bool,
    /// Tempo [lata / sekundę].
    pub speed: f64,
    pub logger: Option<ElementLogger>,
    /// Błąd zapisu z wątku fizyki, czekający na pokazanie w interfejsie.
    pub log_error: Option<String>,
    /// Nazwy planet do pliku z elementami orbitalnymi.
    pub planet_names: Vec<String>,
    /// Ostatni wycinek nie zdążył policzyć zadanego czasu symulacji.
    pub lagging: bool,
}

impl Shared {
    pub fn new(sim: Simulation) -> Self {
        Self {
            sim,
            running: true,
            speed: 1.0,
            logger: None,
            log_error: None,
            planet_names: Vec::new(),
            lagging: false,
        }
    }

    pub fn stop_logging(&mut self, err: std::io::Error) {
        self.logger = None;
        self.log_error = Some(format!("Błąd zapisu: {err}"));
    }

    /// Przesuwa symulację o `duration` lat (co najwyżej `max_steps` kroków), zatrzymując się
    /// dokładnie w chwilach zapisu elementów orbitalnych. Zwraca liczbę wykonanych kroków.
    fn advance(&mut self, duration: f64, max_steps: usize) -> usize {
        let end = self.sim.time + duration;
        let mut steps = 0;
        loop {
            let target = self.logger.as_ref().map_or(end, |l| l.next_time.min(end));
            steps += self.sim.advance(target - self.sim.time, max_steps - steps);
            if let Some(logger) = &mut self.logger
                && self.sim.time >= logger.next_time
                && let Err(e) = logger.record(&self.sim, &self.planet_names)
            {
                self.stop_logging(e);
            }
            if self.sim.time >= end || steps >= max_steps {
                return steps;
            }
        }
    }
}

/// Uchwyt do wątku fizyki; zatrzymuje go przy zamknięciu programu.
pub struct Engine {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn start(shared: Shared) -> Self {
        let shared = Arc::new(Mutex::new(shared));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = thread::Builder::new()
            .name("fizyka".into())
            .spawn({
                let (shared, stop) = (Arc::clone(&shared), Arc::clone(&stop));
                move || run(&shared, &stop)
            })
            .expect("nie udało się uruchomić wątku fizyki");
        Self { shared, stop, thread: Some(thread) }
    }

    pub fn lock(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().ok();
        }
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    // Panika w jednym wątku nie powinna blokować drugiego — stan symulacji pozostaje używalny.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Pętla wątku fizyki: symulacja nadąża za zegarem ściennym, licząc wycinkami nie dłuższymi niż `SLICE`.
fn run(shared: &Mutex<Shared>, stop: &AtomicBool) {
    let mut last = Instant::now();
    // Szacowany czas jednego kroku — z niego wynika, ile kroków zmieści się w wycinku.
    let mut step_cost = Duration::from_micros(10);
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        let dt = (now - last).as_secs_f64().min(0.1);
        last = now;

        let pause = {
            let mut s = lock(shared);
            if s.running {
                let budget = (SLICE.as_secs_f64() / step_cost.as_secs_f64()).clamp(1.0, MAX_STEPS_PER_SLICE as f64);
                let duration = s.speed * dt;
                let (start, timer) = (s.sim.time, Instant::now());
                let steps = s.advance(duration, budget as usize);
                if steps > 0 {
                    step_cost = timer.elapsed() / steps as u32;
                }
                s.lagging = s.sim.time - start < duration * 0.999;
                // Gdy obliczenia nie nadążają, oddajemy blokadę tylko na chwilę.
                if s.lagging { Duration::from_micros(200) } else { Duration::from_millis(1) }
            } else {
                Duration::from_millis(10)
            }
        };
        thread::sleep(pause);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Preset;

    #[test]
    fn runs_in_background_and_stops_on_drop() {
        let mut shared = Shared::new(Simulation::new(&Preset::SolarSystem.config()));
        shared.speed = 10.0;
        let engine = Engine::start(shared);
        thread::sleep(Duration::from_millis(300));
        let t = engine.lock().sim.time;
        assert!(t > 1.0, "po 0,3 s przy 10 lat/s czas symulacji to tylko {t}");

        engine.lock().running = false;
        thread::sleep(Duration::from_millis(50));
        let paused = engine.lock().sim.time;
        thread::sleep(Duration::from_millis(100));
        assert_eq!(engine.lock().sim.time, paused, "pauza nie zatrzymała symulacji");

        let timer = Instant::now();
        drop(engine);
        assert!(timer.elapsed() < Duration::from_millis(500), "wątek fizyki nie zakończył się");
    }
}
