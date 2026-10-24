//! Zapis oskulacyjnych elementów orbitalnych planet do pliku tekstowego w stałych odstępach czasu.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::physics::Simulation;

pub struct ElementLogger {
    writer: BufWriter<File>,
    /// Pełna ścieżka do pliku.
    pub path: PathBuf,
    /// Odstęp między zapisami [lata].
    pub interval: f64,
    /// Czas symulacji najbliższego zapisu [lata].
    pub next_time: f64,
    pub records: usize,
}

impl ElementLogger {
    /// Tworzy (lub nadpisuje) plik i zapisuje nagłówek; pierwszy pomiar nastąpi w chwili `start_time`.
    pub fn create(path: &Path, interval: f64, start_time: f64) -> io::Result<Self> {
        let mut writer = BufWriter::new(File::create(path)?);
        writeln!(writer, "# Oskulacyjne elementy orbitalne planet względem gwiazdy")?;
        writeln!(writer, "# Jednostki: t [lata], a [AU], kąty i, Omega, omega, nu [stopnie]")?;
        writeln!(writer, "# Kolumny rozdzielone tabulatorem")?;
        writeln!(writer, "t\tplaneta\ta\te\ti\tOmega\tomega\tnu")?;
        writer.flush()?;
        Ok(Self {
            writer,
            path: std::path::absolute(path).unwrap_or_else(|_| path.to_owned()),
            interval,
            next_time: start_time,
            records: 0,
        })
    }

    /// Zaznacza w pliku restart symulacji; następny pomiar nastąpi w chwili t = 0.
    pub fn restarted(&mut self) -> io::Result<()> {
        self.next_time = 0.0;
        writeln!(self.writer, "# restart symulacji")?;
        self.writer.flush()
    }

    /// Zapisuje elementy wszystkich planet i wyznacza czas następnego pomiaru.
    /// `planet_names[i]` to nazwa ciała o indeksie `i + 1` (indeks 0 to gwiazda).
    pub fn record(&mut self, sim: &Simulation, planet_names: &[String]) -> io::Result<()> {
        for i in 1..sim.bodies.len() {
            let name = planet_names.get(i - 1).map_or("?", String::as_str);
            let el = sim.orbital_elements(i);
            writeln!(
                self.writer,
                "{:.6}\t{}\t{:.6}\t{:.6}\t{:.4}\t{:.4}\t{:.4}\t{:.4}",
                sim.time, name, el.a, el.e, el.inclination, el.node, el.periapsis, el.anomaly
            )?;
        }
        self.writer.flush()?;
        self.records += 1;
        self.next_time += self.interval;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Preset;

    #[test]
    fn records_elements_at_requested_interval() {
        let cfg = Preset::InnerSolarSystem.config();
        let mut sim = Simulation::new(&cfg);
        let path = std::env::temp_dir().join(format!("planet_animation_test_{}.txt", std::process::id()));
        let names: Vec<String> = cfg.planets.iter().map(|p| p.name.clone()).collect();
        let mut logger = ElementLogger::create(&path, 0.25, 0.0).unwrap();
        while sim.time < 1.0 {
            sim.advance(logger.next_time - sim.time, usize::MAX);
            logger.record(&sim, &names).unwrap();
        }
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();

        let rows: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).skip(1).collect();
        assert_eq!(rows.len(), 5 * cfg.planets.len(), "{text}");
        let earth: Vec<&str> = rows.iter().find(|r| r.starts_with("0.250000\tZiemia")).unwrap().split('\t').collect();
        let a: f64 = earth[2].parse().unwrap();
        assert!((a - 1.0).abs() < 0.01, "a Ziemi = {a}");
    }
}
