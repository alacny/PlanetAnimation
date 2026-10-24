//! Programowy renderer 3D rysujący scenę przez `egui::Painter`:
//! oświetlone sfery (sortowane od najdalszych), ślady orbit, siatka ekliptyki i tło gwiazd.

use eframe::egui::{self, Color32, Mesh, Pos2, Rect, Shape, Stroke, pos2};

use crate::physics::Simulation;
use crate::vec3::Vec3;

pub struct Camera {
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub fov_y: f64,
}

impl Camera {
    pub fn for_scale(scale: f64) -> Self {
        Self { yaw: -1.1, pitch: 0.45, distance: scale * 2.6, fov_y: 45f64.to_radians() }
    }

    pub fn handle_input(&mut self, response: &egui::Response, ui: &egui::Ui) {
        if response.dragged() {
            let d = response.drag_delta();
            self.yaw -= d.x as f64 * 0.006;
            self.pitch = (self.pitch + d.y as f64 * 0.006).clamp(-1.55, 1.55);
        }
        if response.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta().y, i.zoom_delta()));
            self.distance *= (-scroll as f64 * 0.0015).exp() / zoom as f64;
            self.distance = self.distance.clamp(1e-4, 1e5);
        }
    }
}

/// Opcje wizualizacji ustawiane w interfejsie.
pub struct ViewOptions {
    /// Wykładnik kompresji odległości: 1 = skala liniowa, < 1 = zbliża orbity zewnętrzne.
    pub compression: f64,
    /// Mnożnik rozmiarów ciał (nie są w skali — inaczej byłyby niewidoczne).
    pub size_scale: f64,
    pub show_trails: bool,
    pub show_labels: bool,
    pub show_grid: bool,
    pub show_particles: bool,
}

/// Dane opisujące wygląd pojedynczego ciała.
pub struct BodyStyle<'a> {
    pub name: &'a str,
    pub color: [u8; 3],
}

/// Przekształca pozycje fizyczne (AU) na współrzędne sceny, opcjonalnie kompresując odległości od gwiazdy.
struct DisplayMap {
    star: Vec3,
    scale: f64,
    exponent: f64,
}

impl DisplayMap {
    fn relative(&self, rel: Vec3) -> Vec3 {
        let r = rel.length();
        if r == 0.0 || self.exponent == 1.0 {
            return self.star + rel;
        }
        let mapped = self.scale * (r / self.scale).powf(self.exponent);
        self.star + rel * (mapped / r)
    }
}

struct Projector {
    eye: Vec3,
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    focal: f64,
    center: Pos2,
    near: f64,
}

impl Projector {
    fn new(cam: &Camera, target: Vec3, rect: Rect) -> Self {
        let (sp, cp) = cam.pitch.sin_cos();
        let (sy, cy) = cam.yaw.sin_cos();
        let eye = target + Vec3::new(cp * cy, cp * sy, sp) * cam.distance;
        let forward = (target - eye).normalized();
        let right = forward.cross(Vec3::Z).normalized();
        let up = right.cross(forward);
        Self {
            eye,
            right,
            up,
            forward,
            focal: rect.height() as f64 * 0.5 / (cam.fov_y * 0.5).tan(),
            center: rect.center(),
            near: cam.distance * 1e-3,
        }
    }

    /// Rzut punktu na ekran; zwraca pozycję i głębokość (odległość wzdłuż osi widzenia).
    fn project(&self, p: Vec3) -> Option<(Pos2, f64)> {
        let v = p - self.eye;
        let z = v.dot(self.forward);
        (z > self.near).then(|| (self.to_screen(v.dot(self.right) / z, v.dot(self.up) / z), z))
    }

    /// Rzut kierunku leżącego „w nieskończoności” (tło gwiazd).
    fn project_direction(&self, d: Vec3) -> Option<Pos2> {
        let z = d.dot(self.forward);
        (z > 0.05).then(|| self.to_screen(d.dot(self.right) / z, d.dot(self.up) / z))
    }

    fn to_screen(&self, x: f64, y: f64) -> Pos2 {
        pos2(self.center.x + (x * self.focal) as f32, self.center.y - (y * self.focal) as f32)
    }
}

/// Sfera jednostkowa (siatka UV) — wspólna dla wszystkich ciał.
pub struct SphereMesh {
    normals: Vec<Vec3>,
    triangles: Vec<[u32; 3]>,
}

impl SphereMesh {
    pub fn new(stacks: u32, slices: u32) -> Self {
        let mut normals = Vec::new();
        for i in 0..=stacks {
            let theta = std::f64::consts::PI * i as f64 / stacks as f64;
            for j in 0..=slices {
                let phi = std::f64::consts::TAU * j as f64 / slices as f64;
                normals.push(Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos()));
            }
        }
        let mut triangles = Vec::new();
        let row = slices + 1;
        for i in 0..stacks {
            for j in 0..slices {
                let (a, b) = (i * row + j, (i + 1) * row + j);
                if i != 0 {
                    triangles.push([a, b, a + 1]);
                }
                if i != stacks - 1 {
                    triangles.push([a + 1, b, b + 1]);
                }
            }
        }
        Self { normals, triangles }
    }
}

pub struct Scene<'a> {
    pub sim: &'a Simulation,
    pub styles: Vec<BodyStyle<'a>>,
    pub system_scale: f64,
    pub follow: Option<usize>,
}

/// Rysuje scenę i zwraca ekranowe pozycje oraz promienie ciał (do wybierania myszą).
pub fn draw(
    painter: &egui::Painter,
    rect: Rect,
    cam: &Camera,
    opts: &ViewOptions,
    scene: &Scene,
    sphere: &SphereMesh,
) -> Vec<Option<(Pos2, f32)>> {
    let sim = scene.sim;
    painter.rect_filled(rect, 0.0, Color32::from_rgb(4, 5, 12));

    let map = DisplayMap { star: sim.bodies[0].pos, scale: scene.system_scale, exponent: opts.compression };
    let centers: Vec<Vec3> = sim.bodies.iter().map(|b| map.relative(b.pos - map.star)).collect();
    let target = centers[scene.follow.unwrap_or(0).min(centers.len() - 1)];
    let proj = Projector::new(cam, target, rect);

    draw_starfield(painter, &proj);
    if opts.show_grid {
        draw_grid(painter, &proj, &map);
    }
    if opts.show_trails {
        draw_trails(painter, &proj, &map, scene);
    }
    if opts.show_particles {
        draw_particles(painter, &proj, &map, sim);
    }

    let base = scene.system_scale * 0.012 * opts.size_scale;
    let radii: Vec<f64> = sim
        .bodies
        .iter()
        .enumerate()
        .map(|(i, b)| {
            if i == 0 {
                base * 3.0 * b.mass.cbrt().clamp(0.3, 3.0)
            } else {
                base * 0.8 * (b.mass / crate::system::EARTH_MASS_IN_SUNS).cbrt().clamp(0.35, 8.0)
            }
        })
        .collect();

    // Ekranowe położenia i promienie (promień z rzutu punktu na brzegu sfery).
    let screen: Vec<Option<(Pos2, f32)>> = centers
        .iter()
        .zip(&radii)
        .map(|(&c, &r)| {
            let (p, _) = proj.project(c)?;
            let (edge, _) = proj.project(c + proj.right * r)?;
            Some((p, (edge.x - p.x).abs().max(1.5)))
        })
        .collect();

    if let Some((p, r)) = screen[0] {
        draw_glow(painter, p, r, scene.styles[0].color);
    }

    // Trójkąty wszystkich sfer, sortowane od najdalszych (algorytm malarza).
    let mut tris: Vec<(f64, [(Pos2, Color32); 3])> = Vec::new();
    for (i, (&c, &r)) in centers.iter().zip(&radii).enumerate() {
        let light = (centers[0] - c).normalized();
        let [cr, cg, cb] = scene.styles[i].color;
        let shade = |n: Vec3, view: Vec3| -> Color32 {
            let k = if i == 0 {
                // Gwiazda świeci sama; pociemnienie brzegowe tarczy.
                let mu = n.dot(view).max(0.0);
                return mix_to_white([cr, cg, cb], 0.55 * mu, 0.7 + 0.3 * mu);
            } else {
                0.06 + 0.94 * n.dot(light).max(0.0).powf(0.8)
            };
            Color32::from_rgb((cr as f64 * k) as u8, (cg as f64 * k) as u8, (cb as f64 * k) as u8)
        };
        for t in &sphere.triangles {
            let n = (sphere.normals[t[0] as usize] + sphere.normals[t[1] as usize] + sphere.normals[t[2] as usize])
                .normalized();
            if n.dot(proj.eye - (c + n * r)) <= 0.0 {
                continue;
            }
            let mut out = [(Pos2::ZERO, Color32::BLACK); 3];
            let mut depth = 0.0;
            let mut visible = true;
            for (k, &vi) in t.iter().enumerate() {
                let nv = sphere.normals[vi as usize];
                let p = c + nv * r;
                match proj.project(p) {
                    Some((sp, z)) => {
                        out[k] = (sp, shade(nv, (proj.eye - p).normalized()));
                        depth += z;
                    }
                    None => visible = false,
                }
            }
            if visible {
                tris.push((depth, out));
            }
        }
    }
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut mesh = Mesh::default();
    for (_, t) in &tris {
        let idx = mesh.vertices.len() as u32;
        for &(p, col) in t {
            mesh.colored_vertex(p, col);
        }
        mesh.add_triangle(idx, idx + 1, idx + 2);
    }
    painter.add(Shape::mesh(mesh));

    if opts.show_labels {
        for (i, s) in screen.iter().enumerate() {
            if let Some((p, r)) = s {
                painter.text(
                    *p + egui::vec2(r + 4.0, -r - 2.0),
                    egui::Align2::LEFT_BOTTOM,
                    scene.styles[i].name,
                    egui::FontId::proportional(13.0),
                    Color32::from_gray(210),
                );
            }
        }
    }
    screen
}

fn mix_to_white(c: [u8; 3], t: f64, k: f64) -> Color32 {
    let f = |v: u8| ((v as f64 + (255.0 - v as f64) * t) * k).min(255.0) as u8;
    Color32::from_rgb(f(c[0]), f(c[1]), f(c[2]))
}

/// Poświata gwiazdy rysowana addytywnie (alfa = 0 w kolorze premultiplied).
fn draw_glow(painter: &egui::Painter, center: Pos2, radius: f32, color: [u8; 3]) {
    let mut mesh = Mesh::default();
    let segments = 48;
    let outer = radius * 4.0;
    let inner = Color32::from_rgba_premultiplied(color[0] / 2, color[1] / 2, color[2] / 2, 0);
    mesh.colored_vertex(center, inner);
    for s in 0..=segments {
        let a = std::f32::consts::TAU * s as f32 / segments as f32;
        let (sa, ca) = a.sin_cos();
        mesh.colored_vertex(center + egui::vec2(ca, sa) * radius * 0.9, inner);
        mesh.colored_vertex(center + egui::vec2(ca, sa) * outer, Color32::TRANSPARENT);
    }
    for s in 0..segments as u32 {
        let (i0, o0, i1, o1) = (1 + 2 * s, 2 + 2 * s, 3 + 2 * s, 4 + 2 * s);
        mesh.add_triangle(0, i0, i1);
        mesh.add_triangle(i0, o0, i1);
        mesh.add_triangle(i1, o0, o1);
    }
    painter.add(Shape::mesh(mesh));
}

fn draw_starfield(painter: &egui::Painter, proj: &Projector) {
    // Deterministyczny generator, by gwiazdy tła były zawsze w tych samych miejscach.
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..900 {
        let z = rnd() * 2.0 - 1.0;
        let phi = rnd() * std::f64::consts::TAU;
        let s = (1.0 - z * z).sqrt();
        let dir = Vec3::new(s * phi.cos(), s * phi.sin(), z);
        let bright = rnd();
        if let Some(p) = proj.project_direction(dir) {
            let g = (60.0 + 170.0 * bright * bright) as u8;
            painter.circle_filled(p, 0.6 + bright as f32, Color32::from_gray(g));
        }
    }
}

fn draw_grid(painter: &egui::Painter, proj: &Projector, map: &DisplayMap) {
    // Okręgi w płaszczyźnie ekliptyki w „okrągłych” odległościach (1, 2, 5 × 10ⁿ AU).
    let max = map.scale * 1.15;
    let mut step = 10f64.powf((map.scale / 30.0).log10().floor());
    let mut rings = Vec::new();
    while step <= max {
        for m in [1.0, 2.0, 5.0] {
            let r = step * m;
            if r <= max && r >= map.scale / 25.0 {
                rings.push(r);
            }
        }
        step *= 10.0;
    }
    let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(120, 140, 180, 40));
    for r in rings {
        let pts: Vec<Option<Pos2>> = (0..=120)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / 120.0;
                proj.project(map.relative(Vec3::new(r * a.cos(), r * a.sin(), 0.0))).map(|(p, _)| p)
            })
            .collect();
        for w in pts.windows(2) {
            if let [Some(a), Some(b)] = w {
                painter.line_segment([*a, *b], stroke);
            }
        }
        if let Some((p, _)) = proj.project(map.relative(Vec3::new(r, 0.0, 0.0))) {
            let label = if r < 1.0 { format!("{r} AU") } else { format!("{r:.0} AU") };
            painter.text(p, egui::Align2::LEFT_TOP, label, egui::FontId::proportional(10.0), Color32::from_gray(90));
        }
    }
}

/// Cząsteczki gazu jako drobne punkty (rysowane pod ciałami).
fn draw_particles(painter: &egui::Painter, proj: &Projector, map: &DisplayMap, sim: &Simulation) {
    // Jedna siatka z kwadracików zamiast osobnego koła dla każdej cząsteczki — przy setkach
    // tysięcy cząsteczek to rząd wielkości szybciej.
    let color = Color32::from_rgba_unmultiplied(160, 190, 255, 120);
    let size = egui::vec2(2.0, 2.0);
    let mut mesh = Mesh::default();
    for p in &sim.particles {
        if let Some((s, _)) = proj.project(map.relative(p.pos - map.star)) {
            mesh.add_colored_rect(Rect::from_center_size(s, size), color);
        }
    }
    painter.add(Shape::mesh(mesh));
}

fn draw_trails(painter: &egui::Painter, proj: &Projector, map: &DisplayMap, scene: &Scene) {
    for (i, trail) in scene.sim.trails.iter().enumerate().skip(1) {
        let [r, g, b] = scene.styles[i].color;
        let n = trail.points.len();
        let current = scene.sim.bodies[i].pos - scene.sim.bodies[0].pos;
        let pts: Vec<Option<Pos2>> = trail
            .points
            .iter()
            .chain(std::iter::once(&current))
            .map(|&p| proj.project(map.relative(p)).map(|(s, _)| s))
            .collect();
        for (k, w) in pts.windows(2).enumerate() {
            if let [Some(a), Some(c)] = w {
                let alpha = (30.0 + 190.0 * k as f32 / n.max(1) as f32) as u8;
                painter.line_segment([*a, *c], Stroke::new(1.3, Color32::from_rgba_unmultiplied(r, g, b, alpha)));
            }
        }
    }
}
