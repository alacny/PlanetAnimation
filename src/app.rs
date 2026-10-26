//! Aplikacja: panel sterowania (masy, orbity, presety) oraz widok 3D.

use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use crate::engine::{Engine, Shared};
use crate::gas::GasConfig;
use crate::logger::ElementLogger;
use crate::physics::{Simulation, TRAIL_SAMPLES_PER_ORBIT};
use crate::profile_editor::ProfileEditor;
use crate::render::{self, BodyStyle, Camera, Scene, SphereMesh, ViewOptions};
use crate::system::{EARTH_MASS_IN_SUNS, PlanetConfig, Preset, SystemConfig, palette_color};
use crate::video::Recorder;

/// 1 AU/rok w km/s.
const AU_PER_YEAR_IN_KMS: f64 = 4.740_47;
/// Masa Jowisza w masach Ziemi.
const JUPITER_IN_EARTHS: f64 = 317.83;
/// Klatka interfejsu przygotowywana dłużej niż tyle [s] oznacza, że komputer nie nadąża z rysowaniem.
const SLOW_FRAME: f32 = 0.05;
/// Tyle czasu bez przeciążenia potrzeba, by wrócić do płynnej animacji.
const RECOVERY: Duration = Duration::from_secs(5);

/// Decyduje, kiedy odświeżyć obraz: w każdej klatce (płynnie) albo co `sample_interval` sekund —
/// gdy płynna animacja jest wyłączona albo komputer nie nadąża.
struct Pacing {
    /// Czy rysować widok 3D (obliczenia trwają niezależnie od tego).
    animate: bool,
    smooth: bool,
    /// Odstęp między kolejnymi obrazami, gdy animacja nie jest płynna [s].
    sample_interval: f64,
    last_sample: Option<Instant>,
    /// Komputer nie nadąża — animacja przeszła w tryb oszczędny.
    overloaded: bool,
    calm_since: Option<Instant>,
    /// Wygładzony czas przygotowania jednej klatki interfejsu [s].
    frame_time: f32,
}

impl Pacing {
    fn update_load(&mut self, now: Instant, physics_lagging: bool, frame_cpu: Option<f32>) {
        if let Some(cpu) = frame_cpu {
            self.frame_time += (cpu - self.frame_time) * 0.2;
        }
        let rendering_slow = self.animate && self.frame_time > SLOW_FRAME;
        if physics_lagging || rendering_slow {
            self.overloaded = true;
            self.calm_since = None;
        } else if self.overloaded && now - *self.calm_since.get_or_insert(now) >= RECOVERY {
            self.overloaded = false;
        }
    }

    fn smooth_now(&self) -> bool {
        self.animate && self.smooth && !self.overloaded
    }

    fn interval(&self) -> Duration {
        if self.smooth_now() { Duration::ZERO } else { Duration::from_secs_f64(self.sample_interval) }
    }

    /// Czy pora na nowy obraz; jeśli tak, zapamiętuje tę chwilę.
    fn take_sample(&mut self, now: Instant) -> bool {
        let due = self.last_sample.is_none_or(|t| now - t >= self.interval());
        if due {
            self.last_sample = Some(now);
        }
        due
    }

    fn until_next(&self, now: Instant) -> Duration {
        self.last_sample.map_or(Duration::ZERO, |t| (t + self.interval()).saturating_duration_since(now))
    }
}

pub struct PlanetApp {
    preset: Preset,
    config: SystemConfig,
    /// Symulacja liczy się w wątku fizyki.
    engine: Engine,
    /// Kopia stanu symulacji z wątku fizyki, odświeżana co klatkę — z niej rysujemy.
    sim: Simulation,
    camera: Camera,
    view: ViewOptions,
    sphere: SphereMesh,
    running: bool,
    /// Tempo [lata / sekundę].
    speed: f64,
    trail_orbits: f64,
    follow: Option<usize>,
    /// Elementy orbit zmieniono — zaczną działać po restarcie.
    orbits_dirty: bool,
    pacing: Pacing,
    /// W tej klatce pobrano nowy stan symulacji (do nagrywania).
    fresh_frame: bool,
    /// Prostokąt widoku 3D z ostatniej klatki [punkty ekranu].
    viewport_rect: egui::Rect,
    gas: GasConfig,
    /// Parametry rozkładu gazu zmieniono — zaczną działać po ponownym rozsianiu.
    gas_dirty: bool,
    /// Błąd rozsiewania gazu (np. błędny wzór profilu gęstości).
    gas_error: Option<String>,
    profile_editor: ProfileEditor,
    /// Liczba zapisanych pomiarów i pełna ścieżka pliku, gdy zapis trwa (kopia z wątku fizyki).
    log_status: Option<(usize, String)>,
    log_path: String,
    /// Odstęp między zapisami elementów orbitalnych [lata].
    log_interval: f64,
    log_error: Option<String>,
    recorder: Option<Recorder>,
    video_path: String,
    video_fps: u32,
    /// Film w połowie rozdzielczości ekranu — koder nadąża wtedy ok. 3× lepiej.
    video_half_size: bool,
    /// Wynik ostatniego nagrania albo błąd.
    video_message: Option<Result<String, String>>,
}

impl PlanetApp {
    pub fn new() -> Self {
        let preset = Preset::SolarSystem;
        let config = preset.config();
        let sim = Simulation::new(&config);
        let mut app = Self {
            preset,
            engine: Engine::start(Shared::new(sim.clone())),
            sim,
            camera: Camera::for_scale(1.0),
            view: ViewOptions {
                compression: 0.5,
                size_scale: 1.0,
                show_trails: true,
                show_labels: true,
                show_grid: true,
                show_particles: true,
            },
            sphere: SphereMesh::new(18, 28),
            running: true,
            speed: config.speed,
            trail_orbits: 1.0,
            follow: None,
            orbits_dirty: false,
            pacing: Pacing {
                animate: true,
                smooth: true,
                sample_interval: 1.0,
                last_sample: None,
                overloaded: false,
                calm_since: None,
                frame_time: 0.0,
            },
            fresh_frame: false,
            viewport_rect: egui::Rect::NOTHING,
            gas: GasConfig::for_system(&config),
            gas_dirty: false,
            gas_error: None,
            profile_editor: ProfileEditor::default(),
            log_status: None,
            log_path: "elementy_orbitalne.txt".to_owned(),
            log_interval: 0.1,
            log_error: None,
            recorder: None,
            video_path: "animacja.webm".to_owned(),
            video_fps: 20,
            video_half_size: true,
            video_message: None,
            config,
        };
        app.restart(true);
        app
    }

    fn system_scale(&self) -> f64 {
        self.config.planets.iter().map(|p| p.a * (1.0 + p.e)).fold(0.0, f64::max).max(0.01)
    }

    fn restart(&mut self, reset_camera: bool) {
        let mut sim = Simulation::new(&self.config);
        sim.trail_capacity = self.trail_capacity();
        self.gas_error = None;
        if self.gas.enabled {
            let particles = self.gas.scatter(&sim);
            self.gas_error = particles.as_ref().err().cloned();
            sim.set_particles(particles.unwrap_or_default(), self.gas.removal_bounds(), self.gas.drag);
        }
        self.orbits_dirty = false;
        self.gas_dirty = false;

        let mut shared = self.engine.lock();
        shared.sim = sim;
        if let Some(logger) = &mut shared.logger
            && let Err(e) = logger.restarted()
        {
            shared.stop_logging(e);
        }
        self.sim.clone_from(&shared.sim);
        drop(shared);

        if self.follow.is_some_and(|f| f >= self.sim.bodies.len()) {
            self.follow = None;
        }
        if reset_camera {
            self.camera = Camera::for_scale(self.system_scale());
        }
    }

    /// Rozsiewa cząsteczki od nowa (albo je usuwa, gdy gaz jest wyłączony).
    fn scatter_gas(&mut self) {
        let mut shared = self.engine.lock();
        let particles = if self.gas.enabled { self.gas.scatter(&shared.sim) } else { Ok(Vec::new()) };
        let particles = match particles {
            Ok(particles) => particles,
            Err(err) => {
                self.gas_error = Some(err);
                return;
            }
        };
        self.gas_error = None;
        shared.sim.set_particles(particles, self.gas.removal_bounds(), self.gas.drag);
        self.sim.clone_from(&shared.sim);
        self.gas_dirty = false;
    }

    /// Wymiana stanu z wątkiem fizyki: przekazuje ustawienia, odczytuje stan zapisu, a gdy
    /// `snapshot`, pobiera też kopię symulacji do rysowania. Zwraca, czy fizyka nie nadąża.
    fn sync_with_engine(&mut self, snapshot: bool) -> bool {
        let mut shared = self.engine.lock();
        shared.running = self.running;
        shared.speed = self.speed;
        shared.planet_names.clear();
        shared.planet_names.extend(self.config.planets.iter().map(|p| p.name.clone()));
        if snapshot {
            self.sim.clone_from(&shared.sim);
        }
        self.log_status = shared.logger.as_ref().map(|l| (l.records, l.path.display().to_string()));
        if let Some(err) = shared.log_error.take() {
            self.log_error = Some(err);
        }
        shared.running && shared.lagging
    }

    fn trail_capacity(&self) -> usize {
        (TRAIL_SAMPLES_PER_ORBIT * self.trail_orbits).round() as usize
    }

    fn apply_trail_length(&mut self) {
        let capacity = self.trail_capacity();
        self.engine.lock().sim.trail_capacity = capacity;
        self.sim.trail_capacity = capacity;
    }

    fn body_name(&self, i: usize) -> &str {
        if i == 0 { &self.config.star_name } else { &self.config.planets[i - 1].name }
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.heading("☉ Układ planetarny");
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("Układ:");
            let mut chosen = self.preset;
            egui::ComboBox::from_id_salt("preset")
                .selected_text(chosen.label())
                .width(210.0)
                .show_ui(ui, |ui| {
                    for p in Preset::ALL {
                        ui.selectable_value(&mut chosen, p, p.label());
                    }
                });
            if chosen != self.preset {
                self.preset = chosen;
                self.config = chosen.config();
                self.speed = self.config.speed;
                self.follow = None;
                self.gas.fit_to(&self.config);
                self.restart(true);
            }
        });

        ui.separator();
        self.mass_editor(ui);
        ui.separator();
        self.orbit_editor(ui);
        ui.separator();
        self.simulation_controls(ui);
        ui.separator();
        self.animation_controls(ui);
        ui.separator();
        self.gas_controls(ui);
        ui.separator();
        self.log_controls(ui);
        ui.separator();
        self.view_controls(ui);
        ui.separator();
        self.info(ui);
    }

    fn mass_editor(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Masy ciał").strong());
        ui.label(
            RichText::new("Zmiany mas działają natychmiast, także w trakcie ruchu.")
                .small()
                .weak(),
        );
        let mut changed: Vec<(usize, f64)> = Vec::new();
        egui::Grid::new("masses").num_columns(3).striped(true).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.color_edit_button_srgb(&mut self.config.star_color);
                ui.text_edit_singleline(&mut self.config.star_name).on_hover_text("Nazwa gwiazdy");
            });
            let m = &mut self.config.star_mass;
            let speed = (*m * 0.01).max(1e-4);
            if ui.add(egui::DragValue::new(m).speed(speed).range(0.001..=1000.0).max_decimals(4)).changed() {
                changed.push((0, *m));
            }
            ui.label("M☉");
            ui.end_row();

            for (i, p) in self.config.planets.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut p.color);
                    ui.label(&p.name);
                });
                let speed = (p.mass_earth * 0.01).max(1e-3);
                let resp = ui.add(
                    egui::DragValue::new(&mut p.mass_earth).speed(speed).range(0.0..=1e7).max_decimals(4),
                );
                if resp.changed() {
                    changed.push((i + 1, p.mass_earth * EARTH_MASS_IN_SUNS));
                }
                ui.label("M⊕").on_hover_text(format!(
                    "= {:.4} M♃ = {:.3e} M☉",
                    p.mass_earth / JUPITER_IN_EARTHS,
                    p.mass_earth * EARTH_MASS_IN_SUNS
                ));
                ui.end_row();
            }
        });
        if !changed.is_empty() {
            let mut shared = self.engine.lock();
            for (i, m) in changed {
                shared.sim.set_mass(i, m);
            }
            self.sim.clone_from(&shared.sim);
        }
    }

    fn orbit_editor(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Orbity początkowe").strong());
        ui.label(RichText::new("Działają po restarcie symulacji.").small().weak());

        let mut remove = None;
        for (i, p) in self.config.planets.iter_mut().enumerate() {
            let [r, g, b] = p.color;
            egui::CollapsingHeader::new(RichText::new(&p.name).color(Color32::from_rgb(r, g, b)))
                .id_salt(("planet", i))
                .show(ui, |ui| {
                    let before = (p.a, p.e, p.inclination, p.node, p.periapsis, p.anomaly);
                    egui::Grid::new(("orbit", i)).num_columns(2).show(ui, |ui| {
                        ui.label("Nazwa");
                        ui.text_edit_singleline(&mut p.name);
                        ui.end_row();
                        ui.label("Półoś a [AU]");
                        let s = p.a * 0.005;
                        ui.add(egui::DragValue::new(&mut p.a).speed(s).range(0.001..=1e4));
                        ui.end_row();
                        ui.label("Mimośród e");
                        ui.add(egui::DragValue::new(&mut p.e).speed(0.002).range(0.0..=0.95));
                        ui.end_row();
                        ui.label("Nachylenie i [°]");
                        ui.add(egui::DragValue::new(&mut p.inclination).speed(0.2).range(-180.0..=180.0));
                        ui.end_row();
                        ui.label("Węzeł Ω [°]");
                        ui.add(egui::DragValue::new(&mut p.node).speed(1.0).range(-360.0..=360.0));
                        ui.end_row();
                        ui.label("Perycentrum ω [°]");
                        ui.add(egui::DragValue::new(&mut p.periapsis).speed(1.0).range(-360.0..=360.0));
                        ui.end_row();
                        ui.label("Anomalia ν [°]");
                        ui.add(egui::DragValue::new(&mut p.anomaly).speed(1.0).range(0.0..=360.0));
                        ui.end_row();
                    });
                    if before != (p.a, p.e, p.inclination, p.node, p.periapsis, p.anomaly) {
                        self.orbits_dirty = true;
                    }
                    if ui.button("🗑 Usuń planetę").clicked() {
                        remove = Some(i);
                    }
                });
        }

        if let Some(i) = remove {
            self.config.planets.remove(i);
            self.follow = None;
            self.restart(false);
        }

        ui.horizontal(|ui| {
            if ui.button("➕ Dodaj planetę").clicked() {
                let n = self.config.planets.len();
                let a = self.config.planets.iter().map(|p| p.a).fold(0.0, f64::max).max(0.5) * 1.5;
                self.config.planets.push(PlanetConfig {
                    name: format!("Planeta {}", n + 1),
                    mass_earth: 1.0,
                    a,
                    e: 0.0,
                    inclination: 0.0,
                    node: 0.0,
                    periapsis: 0.0,
                    anomaly: (n as f64 * 137.5) % 360.0,
                    color: palette_color(n),
                });
                self.preset = Preset::Custom;
                self.restart(false);
                self.camera.distance = self.camera.distance.max(self.system_scale() * 2.6);
            }
            let restart = egui::Button::new(if self.orbits_dirty { "↻ Zastosuj i restartuj" } else { "↻ Restart" })
                .fill(if self.orbits_dirty { Color32::from_rgb(120, 70, 20) } else { Color32::TRANSPARENT });
            if ui.add(restart).clicked() {
                self.restart(false);
            }
        });
    }

    fn simulation_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Symulacja").strong());
        ui.horizontal(|ui| {
            let label = if self.running { "⏸ Pauza" } else { "▶ Start" };
            if ui.button(label).on_hover_text("Skrót: spacja").clicked() {
                self.running = !self.running;
            }
            ui.label(format!("t = {:.3} lat", self.sim.time));
        });
        ui.add(
            egui::Slider::new(&mut self.speed, 1e-4..=200.0)
                .logarithmic(true)
                .text("lat / s"),
        );
        if ui.add(egui::Slider::new(&mut self.trail_orbits, 0.0..=4.0).text("ślad [obiegi]")).changed() {
            self.apply_trail_length();
        }
        if self.pacing.overloaded {
            ui.colored_label(
                Color32::from_rgb(230, 160, 60),
                format!(
                    "⚠ Komputer nie nadąża — obraz odświeżany co {} s. Zmniejsz tempo lub wyłącz animację.",
                    self.pacing.sample_interval
                ),
            );
        }
    }

    fn animation_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Animacja i nagrywanie").strong());
        let p = &mut self.pacing;
        ui.horizontal(|ui| {
            ui.checkbox(&mut p.animate, "animacja")
                .on_hover_text("Wyłączona — obliczenia trwają dalej, a komputer ma więcej mocy na fizykę.");
            ui.add_enabled(p.animate, egui::Checkbox::new(&mut p.smooth, "płynna"))
                .on_hover_text("Wyłączona — obraz odświeżany co podany czas zamiast w każdej klatce.");
        });
        ui.horizontal(|ui| {
            ui.label("Obraz co");
            ui.add(egui::DragValue::new(&mut p.sample_interval).speed(0.05).range(0.1..=10.0).suffix(" s"));
            ui.label(RichText::new("(gdy nie płynnie)").small().weak());
        });

        ui.add_space(4.0);
        let recording = self.recorder.is_some();
        egui::Grid::new("video").num_columns(2).show(ui, |ui| {
            ui.label("Film");
            ui.add_enabled(!recording, egui::TextEdit::singleline(&mut self.video_path));
            ui.end_row();
            ui.label("");
            ui.add_enabled_ui(!recording, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut self.video_fps).range(5..=60).suffix(" kl./s"));
                    ui.checkbox(&mut self.video_half_size, "½ rozdzielczości")
                        .on_hover_text("Mniejszy film koduje się ok. 3× szybciej — mniej pominiętych klatek.");
                });
            });
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if recording {
                if ui.button("⏹ Zakończ nagrywanie").clicked() {
                    self.stop_recording();
                }
            } else if ui.button("⏺ Nagrywaj film").on_hover_text("WebM z kodekiem AV1 (otwarty, bez opłat)").clicked() {
                self.start_recording(ui.ctx().pixels_per_point());
            }
        });
        if let Some(rec) = &self.recorder {
            let (encoded, dropped) = rec.stats();
            ui.label(
                RichText::new(format!("⏺ {}×{}: zapisano {encoded} klatek, pominięto {dropped}", rec.size[0], rec.size[1]))
                    .small()
                    .color(Color32::from_rgb(230, 90, 80)),
            )
            .on_hover_text("Klatki są pomijane, gdy koder nie nadąża — film zachowuje rzeczywiste tempo.");
            if !self.pacing.animate {
                ui.label(RichText::new("Animacja wyłączona — do filmu nie trafiają nowe klatki.").small().weak());
            }
        }
        match &self.video_message {
            Some(Ok(msg)) => {
                ui.label(RichText::new(msg).small().weak());
            }
            Some(Err(err)) => {
                ui.colored_label(Color32::from_rgb(230, 100, 80), err);
            }
            None => {}
        }
    }

    fn start_recording(&mut self, pixels_per_point: f32) {
        let size = self.viewport_rect.size() * pixels_per_point;
        let area = [size.x.round() as usize, size.y.round() as usize];
        let scale = if self.video_half_size { 2 } else { 1 };
        match Recorder::start(Path::new(&self.video_path), area, scale, self.video_fps) {
            Ok(rec) => {
                self.recorder = Some(rec);
                self.video_message = None;
            }
            Err(e) => self.video_message = Some(Err(format!("Nie można nagrywać: {e}"))),
        }
    }

    fn stop_recording(&mut self) {
        if let Some(rec) = self.recorder.take() {
            let (path, (encoded, _)) = (rec.path.display().to_string(), rec.stats());
            self.video_message = Some(match rec.finish() {
                Ok(()) => Ok(format!("Zapisano film ({encoded} klatek): {path}")),
                Err(e) => Err(format!("Błąd zapisu filmu: {e}")),
            });
        }
    }

    /// Zleca zrzut ekranu dla filmu, gdy w tej klatce narysowano nowy stan i pora na kolejną klatkę.
    fn capture_video_frame(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Some(rec) = &mut self.recorder
            && self.pacing.animate
            && self.fresh_frame
            && rec.wants_frame(now)
        {
            let origin = self.viewport_rect.min * ctx.pixels_per_point();
            ctx.request_screenshot(rec.capture(now, [origin.x.round() as usize, origin.y.round() as usize]));
        }
    }

    fn gas_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Gaz (cząsteczki)").strong());
        if ui.checkbox(&mut self.gas.enabled, "Rozsiej losowo cząsteczki gazu").changed() {
            self.scatter_gas();
        }
        if !self.gas.enabled {
            return;
        }
        ui.label(
            RichText::new("Cząsteczki krążą wokół gwiazdy, przyciągają planety i hamują je w ich sferze Hilla.")
                .small()
                .weak(),
        );

        let g = &mut self.gas;
        let layout_before = (g.count, g.inner, g.outer, g.thickness);
        let mut coupling_changed = false;
        egui::Grid::new("gas").num_columns(2).show(ui, |ui| {
            ui.label("Liczba cząsteczek");
            let speed = (g.count as f64 * 0.01).max(1.0);
            ui.add(egui::DragValue::new(&mut g.count).speed(speed).range(10..=1_000_000))
                .on_hover_text("Więcej cząsteczek = wolniejsza symulacja");
            ui.end_row();
            ui.label("Masa gazu [M⊕]");
            let s = (g.total_mass_earth * 0.01).max(0.01);
            coupling_changed |= ui
                .add(egui::DragValue::new(&mut g.total_mass_earth).speed(s).range(0.0..=1e5).max_decimals(3))
                .changed();
            ui.end_row();
            ui.label("Promień wewn. [AU]");
            let s = g.inner * 0.005;
            ui.add(egui::DragValue::new(&mut g.inner).speed(s).range(0.001..=1e4));
            ui.end_row();
            ui.label("Promień zewn. [AU]");
            let s = g.outer * 0.005;
            ui.add(egui::DragValue::new(&mut g.outer).speed(s).range(0.001..=1e4));
            ui.end_row();
            ui.label("Grubość dysku [°]");
            ui.add(egui::DragValue::new(&mut g.thickness).speed(0.1).range(0.0..=30.0));
            ui.end_row();
            ui.label("Siła oporu");
            coupling_changed |= ui
                .add(egui::DragValue::new(&mut g.drag).speed(0.02).range(0.0..=10.0))
                .on_hover_text("0 = tylko grawitacja; więcej = gaz silniej hamuje planety")
                .changed();
            ui.end_row();
        });
        if layout_before != (g.count, g.inner, g.outer, g.thickness) {
            self.gas_dirty = true;
        }
        ui.add_space(4.0);
        let radii = self.gas.radii();
        if self.profile_editor.show(ui, &mut self.gas.profile, radii) {
            self.gas_dirty = true;
        }
        if coupling_changed {
            let mut shared = self.engine.lock();
            let n = shared.sim.particles.len() + shared.sim.particles_lost;
            shared.sim.set_gas_coupling(self.gas.particle_mass(n), self.gas.drag);
            self.sim.clone_from(&shared.sim);
        }

        ui.horizontal(|ui| {
            let button = egui::Button::new("🎲 Rozsiej ponownie")
                .fill(if self.gas_dirty { Color32::from_rgb(120, 70, 20) } else { Color32::TRANSPARENT });
            if ui.add(button).clicked() {
                self.scatter_gas();
            }
            ui.label(format!("{} cząsteczek (utracone: {})", self.sim.particles.len(), self.sim.particles_lost))
                .on_hover_text("Cząsteczki pochłonięte przez gwiazdę lub wyrzucone z układu są usuwane");
        });
        if let Some(err) = &self.gas_error {
            ui.colored_label(Color32::from_rgb(230, 110, 90), format!("Nie rozsiano gazu: {err}"));
        }
    }

    fn log_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Zapis elementów orbitalnych").strong());
        let mut on = self.log_status.is_some();
        if ui.checkbox(&mut on, "Zapisuj do pliku tekstowego").changed() {
            let mut shared = self.engine.lock();
            shared.logger = None;
            if on {
                match ElementLogger::create(Path::new(&self.log_path), self.log_interval, shared.sim.time) {
                    Ok(logger) => {
                        self.log_status = Some((0, logger.path.display().to_string()));
                        shared.logger = Some(logger);
                        self.log_error = None;
                    }
                    Err(e) => self.log_error = Some(format!("Nie można utworzyć pliku: {e}")),
                }
            } else {
                self.log_status = None;
            }
        }
        egui::Grid::new("log").num_columns(2).show(ui, |ui| {
            ui.label("Plik");
            ui.add_enabled(self.log_status.is_none(), egui::TextEdit::singleline(&mut self.log_path));
            ui.end_row();
            ui.label("Co ile [lata]");
            let s = self.log_interval * 0.01;
            let resp = ui.add(egui::DragValue::new(&mut self.log_interval).speed(s).range(1e-3..=1e4));
            if resp.changed()
                && let Some(logger) = &mut self.engine.lock().logger
            {
                logger.interval = self.log_interval;
            }
            ui.end_row();
        });
        if let Some((records, path)) = &self.log_status {
            ui.label(RichText::new(format!("Zapisano {records} pomiarów do:")).small());
            ui.label(RichText::new(path).small().weak());
        }
        if let Some(err) = &self.log_error {
            ui.colored_label(Color32::from_rgb(230, 100, 80), err);
        }
    }

    fn view_controls(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Widok").strong());
        ui.add(egui::Slider::new(&mut self.view.compression, 0.25..=1.0).text("kompresja odległości"))
            .on_hover_text("1.0 = prawdziwe proporcje odległości; mniej = orbity zewnętrzne bliżej");
        ui.add(egui::Slider::new(&mut self.view.size_scale, 0.1..=4.0).logarithmic(true).text("rozmiar ciał"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.view.show_trails, "ślady");
            ui.checkbox(&mut self.view.show_labels, "nazwy");
            ui.checkbox(&mut self.view.show_grid, "siatka");
            ui.checkbox(&mut self.view.show_particles, "gaz");
        });
        ui.horizontal(|ui| {
            ui.label("Kamera śledzi:");
            let current = self.follow.map_or("środek układu".to_owned(), |i| self.body_name(i).to_owned());
            let mut follow = self.follow;
            egui::ComboBox::from_id_salt("follow").selected_text(current).show_ui(ui, |ui| {
                ui.selectable_value(&mut follow, None, "środek układu");
                for i in 0..self.sim.bodies.len() {
                    ui.selectable_value(&mut follow, Some(i), self.body_name(i));
                }
            });
            self.follow = follow;
        });
        if ui.button("Reset kamery").clicked() {
            self.camera = Camera::for_scale(self.system_scale());
            self.follow = None;
        }
    }

    fn info(&self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Stan").strong());
        egui::Grid::new("info").num_columns(6).striped(true).show(ui, |ui| {
            ui.label("ciało");
            ui.label("r [AU]").on_hover_text("Odległość od gwiazdy");
            ui.label("v [km/s]").on_hover_text("Prędkość względem gwiazdy");
            ui.label("a [AU]").on_hover_text("Wielka półoś orbity (ujemna dla orbity otwartej)");
            ui.label("e").on_hover_text("Mimośród orbity");
            ui.label("i [°]").on_hover_text("Nachylenie (inklinacja) orbity");
            ui.end_row();
            for i in 1..self.sim.bodies.len() {
                let (r, v) = self.sim.relative_to_star(i);
                let el = self.sim.orbital_elements(i);
                ui.label(self.body_name(i));
                ui.label(format!("{r:.3}"));
                ui.label(format!("{:.2}", v * AU_PER_YEAR_IN_KMS));
                ui.label(format!("{:.3}", el.a));
                ui.label(format!("{:.4}", el.e));
                ui.label(format!("{:.2}", el.inclination));
                ui.end_row();
            }
        });
        ui.label(RichText::new("a, e, i — oskulacyjne elementy orbity względem gwiazdy").small().weak());
        ui.label(
            RichText::new(format!("Błąd energii: {:.2e}", self.sim.energy_drift()))
                .small()
                .weak(),
        )
        .on_hover_text(
            "Względna zmiana energii planet i gwiazdy — miara dokładności całkowania. \
             Z gazem rośnie także fizycznie, bo gaz wymienia energię z planetami.",
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new("Mysz: przeciągnij — obrót, kółko — zoom, kliknij ciało — śledź je. Spacja — pauza.")
                .small()
                .weak(),
        );
    }

    fn viewport(&mut self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.viewport_rect = rect;
        if !self.pacing.animate {
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_rgb(4, 5, 12));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("Animacja wyłączona — obliczenia trwają (t = {:.3} lat)", self.sim.time),
                egui::FontId::proportional(16.0),
                Color32::from_gray(150),
            );
            return;
        }
        self.camera.handle_input(&response, ui);

        let mut styles = vec![BodyStyle { name: &self.config.star_name, color: self.config.star_color }];
        styles.extend(self.config.planets.iter().map(|p| BodyStyle { name: &p.name, color: p.color }));
        // Po dodaniu/usunięciu planety symulacja jest od razu restartowana, więc liczby się zgadzają.
        styles.truncate(self.sim.bodies.len());

        let scene = Scene { sim: &self.sim, styles, system_scale: self.system_scale(), follow: self.follow };
        let painter = ui.painter_at(rect);
        let screen = render::draw(&painter, rect, &self.camera, &self.view, &scene, &self.sphere);

        if response.clicked()
            && let Some(click) = response.interact_pointer_pos()
        {
            let hit = screen
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.map(|(p, r)| (i, p.distance(click), r)))
                .filter(|&(_, d, r)| d <= r + 8.0)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((i, _, _)) = hit {
                self.follow = Some(i);
            }
        }
    }
}

impl eframe::App for PlanetApp {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // Klawisz jest pochłaniany, by nie „klikał” też przycisku, który ma fokus; w polach tekstowych działa normalnie.
        if !ctx.text_edit_focused() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
            self.running = !self.running;
        }
        let now = Instant::now();
        self.fresh_frame = self.pacing.take_sample(now);
        let lagging = self.sync_with_engine(self.fresh_frame);
        self.pacing.update_load(now, lagging, frame.info().cpu_usage);
        if self.running {
            if self.pacing.smooth_now() {
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(self.pacing.until_next(now));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("controls").default_size(360.0).resizable(true).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.controls(ui));
        });
        egui::CentralPanel::no_frame().show(ui, |ui| self.viewport(ui));
        self.capture_video_frame(ui.ctx());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pacing() -> Pacing {
        Pacing {
            animate: true,
            smooth: true,
            sample_interval: 1.0,
            last_sample: None,
            overloaded: false,
            calm_since: None,
            frame_time: 0.0,
        }
    }

    #[test]
    fn falls_back_to_sampling_when_overloaded_and_recovers_after_calm() {
        let mut p = pacing();
        let t0 = Instant::now();
        assert!(p.smooth_now());

        p.update_load(t0, true, None);
        assert!(!p.smooth_now(), "przeciążenie powinno wyłączyć płynną animację");
        assert!(p.take_sample(t0));
        assert!(!p.take_sample(t0 + Duration::from_millis(500)), "obraz częściej niż co 1 s");
        assert!(p.take_sample(t0 + Duration::from_millis(1000)));

        p.update_load(t0 + Duration::from_secs(1), false, None);
        p.update_load(t0 + Duration::from_secs(3), false, None);
        assert!(p.overloaded, "powrót do płynności przed upływem 5 s spokoju");
        p.update_load(t0 + Duration::from_secs(6), false, None);
        assert!(p.smooth_now());
    }

    #[test]
    fn slow_rendering_counts_as_overload() {
        let mut p = pacing();
        let t0 = Instant::now();
        for _ in 0..20 {
            p.update_load(t0, false, Some(0.2));
        }
        assert!(p.overloaded);
    }

    #[test]
    fn disabling_smooth_animation_samples_every_interval() {
        let mut p = pacing();
        p.smooth = false;
        let t0 = Instant::now();
        assert!(p.take_sample(t0));
        assert!(!p.take_sample(t0 + Duration::from_millis(900)));
        assert_eq!(p.until_next(t0 + Duration::from_millis(900)), Duration::from_millis(100));
    }
}
