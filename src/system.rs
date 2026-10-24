//! Opis układu planetarnego (dane wejściowe od użytkownika) oraz gotowe presety.
//!
//! Jednostki: odległość w AU, masa gwiazdy w masach Słońca (M☉),
//! masa planet w masach Ziemi (M⊕), kąty w stopniach.

/// Masa Ziemi wyrażona w masach Słońca.
pub const EARTH_MASS_IN_SUNS: f64 = 3.003_489e-6;

#[derive(Clone, Debug)]
pub struct PlanetConfig {
    pub name: String,
    /// Masa w masach Ziemi.
    pub mass_earth: f64,
    /// Wielka półoś orbity [AU].
    pub a: f64,
    /// Mimośród.
    pub e: f64,
    /// Nachylenie orbity [°].
    pub inclination: f64,
    /// Długość węzła wstępującego Ω [°].
    pub node: f64,
    /// Argument perycentrum ω [°].
    pub periapsis: f64,
    /// Początkowa anomalia prawdziwa ν [°].
    pub anomaly: f64,
    pub color: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct SystemConfig {
    pub star_name: String,
    /// Masa gwiazdy w masach Słońca.
    pub star_mass: f64,
    pub star_color: [u8; 3],
    pub planets: Vec<PlanetConfig>,
    /// Domyślne tempo symulacji [lata / sekundę].
    pub speed: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    SolarSystem,
    InnerSolarSystem,
    Trappist1,
    HeavyGiant,
    Custom,
}

impl Preset {
    pub const ALL: [Preset; 5] = [
        Preset::SolarSystem,
        Preset::InnerSolarSystem,
        Preset::Trappist1,
        Preset::HeavyGiant,
        Preset::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Preset::SolarSystem => "Układ Słoneczny",
            Preset::InnerSolarSystem => "Wewnętrzny Układ Słoneczny",
            Preset::Trappist1 => "TRAPPIST-1",
            Preset::HeavyGiant => "Eksperyment: masywny olbrzym",
            Preset::Custom => "Własny układ",
        }
    }

    pub fn config(self) -> SystemConfig {
        match self {
            Preset::SolarSystem => solar_system(),
            Preset::InnerSolarSystem => {
                let mut s = solar_system();
                s.planets.truncate(4);
                s.speed = 0.25;
                s
            }
            Preset::Trappist1 => trappist1(),
            Preset::HeavyGiant => heavy_giant(),
            Preset::Custom => custom(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn planet(
    name: &str,
    mass_earth: f64,
    a: f64,
    e: f64,
    inclination: f64,
    node: f64,
    periapsis: f64,
    mean_longitude: f64,
    color: [u8; 3],
) -> PlanetConfig {
    PlanetConfig {
        name: name.to_owned(),
        mass_earth,
        a,
        e,
        inclination,
        node,
        periapsis,
        // Dla małych mimośrodów anomalia prawdziwa ≈ średnia: ν ≈ L − Ω − ω.
        anomaly: (mean_longitude - node - periapsis).rem_euclid(360.0),
        color,
    }
}

/// Elementy orbitalne na epokę J2000.
fn solar_system() -> SystemConfig {
    SystemConfig {
        star_name: "Słońce".into(),
        star_mass: 1.0,
        star_color: [255, 214, 120],
        speed: 1.0,
        planets: vec![
            planet("Merkury", 0.0553, 0.387, 0.2056, 7.00, 48.33, 29.12, 252.25, [170, 160, 150]),
            planet("Wenus", 0.815, 0.723, 0.0068, 3.39, 76.68, 54.88, 181.98, [230, 200, 140]),
            planet("Ziemia", 1.0, 1.000, 0.0167, 0.00, -11.26, 114.21, 100.46, [80, 140, 255]),
            planet("Mars", 0.107, 1.524, 0.0934, 1.85, 49.56, 286.50, 355.45, [220, 100, 60]),
            planet("Jowisz", 317.8, 5.203, 0.0484, 1.30, 100.46, 273.87, 34.40, [215, 175, 130]),
            planet("Saturn", 95.2, 9.537, 0.0539, 2.49, 113.67, 339.39, 49.94, [235, 215, 160]),
            planet("Uran", 14.5, 19.19, 0.0473, 0.77, 74.01, 96.99, 313.23, [150, 220, 230]),
            planet("Neptun", 17.1, 30.07, 0.0086, 1.77, 131.78, 273.19, 304.88, [80, 110, 240]),
        ],
    }
}

fn trappist1() -> SystemConfig {
    let p = |name: &str, m: f64, a: f64, l: f64, c: [u8; 3]| planet(name, m, a, 0.005, 0.1, 0.0, 0.0, l, c);
    SystemConfig {
        star_name: "TRAPPIST-1".into(),
        star_mass: 0.0898,
        star_color: [255, 120, 70],
        speed: 0.01,
        planets: vec![
            p("b", 1.374, 0.01154, 0.0, [200, 150, 120]),
            p("c", 1.308, 0.01580, 60.0, [210, 180, 140]),
            p("d", 0.388, 0.02227, 130.0, [190, 190, 170]),
            p("e", 0.692, 0.02925, 200.0, [90, 150, 220]),
            p("f", 1.039, 0.03849, 260.0, [110, 170, 200]),
            p("g", 1.321, 0.04683, 320.0, [140, 160, 210]),
            p("h", 0.326, 0.06189, 30.0, [180, 200, 220]),
        ],
    }
}

fn heavy_giant() -> SystemConfig {
    let mut s = solar_system();
    s.planets.truncate(5);
    s.planets[4].name = "Super-Jowisz".into();
    s.planets[4].mass_earth = 30_000.0;
    s.planets[4].a = 3.0;
    s.speed = 0.5;
    s
}

fn custom() -> SystemConfig {
    SystemConfig {
        star_name: "Gwiazda".into(),
        star_mass: 1.0,
        star_color: [255, 230, 180],
        speed: 0.5,
        planets: vec![
            planet("Planeta 1", 1.0, 1.0, 0.05, 0.0, 0.0, 0.0, 0.0, [90, 160, 255]),
            planet("Planeta 2", 50.0, 2.5, 0.1, 5.0, 40.0, 0.0, 180.0, [230, 170, 110]),
        ],
    }
}

/// Paleta kolorów dla nowo dodawanych planet.
pub fn palette_color(i: usize) -> [u8; 3] {
    const COLORS: [[u8; 3]; 8] = [
        [90, 160, 255],
        [230, 170, 110],
        [140, 220, 140],
        [220, 110, 110],
        [200, 140, 230],
        [240, 220, 120],
        [120, 220, 220],
        [240, 150, 200],
    ];
    COLORS[i % COLORS.len()]
}
