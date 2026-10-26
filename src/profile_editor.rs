//! Edytor początkowego profilu gęstości dysku: wybór rodzaju profilu i wykres Σ(r),
//! na którym krzywą Béziera kształtuje się, przeciągając punkty i ich uchwyty.

use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, pos2, vec2};

use crate::profile::{BezierCurve, DensityProfile, Point, ProfileKind, Side, TABLE_SIZE, function_names};

/// Odległość [px], z jakiej można chwycić punkt lub uchwyt.
const HIT_RADIUS: f32 = 9.0;
const PLOT_HEIGHT: f32 = 170.0;
/// Liczba punktów krzywej tworzonej z profilu potęgowego lub wzoru.
const CONVERTED_POINTS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Target {
    Anchor(usize),
    Handle(usize, Side),
}

impl Target {
    fn anchor(self) -> usize {
        match self {
            Target::Anchor(i) | Target::Handle(i, _) => i,
        }
    }
}

#[derive(Default)]
pub struct ProfileEditor {
    selected: Option<usize>,
    dragging: Option<Target>,
    /// Obszar wykresu z ostatniej klatki — testy przeliczają według niego współrzędne kursora.
    #[cfg(test)]
    plot_rect: Option<Rect>,
}

impl ProfileEditor {
    /// Rysuje edytor profilu dla dysku o promieniach `lo`–`hi` [AU]. Zwraca, czy profil się zmienił.
    pub fn show(&mut self, ui: &mut egui::Ui, profile: &mut DensityProfile, (lo, hi): (f64, f64)) -> bool {
        let before = profile.clone();
        ui.horizontal(|ui| {
            ui.label("Profil gęstości");
            egui::ComboBox::from_id_salt("profile_kind").selected_text(profile.kind.label()).show_ui(ui, |ui| {
                for kind in ProfileKind::ALL {
                    ui.selectable_value(&mut profile.kind, kind, kind.label());
                }
            });
        });
        match profile.kind {
            ProfileKind::PowerLaw => {
                ui.horizontal(|ui| {
                    ui.label("Wykładnik p");
                    ui.add(egui::DragValue::new(&mut profile.exponent).speed(0.02).range(-3.0..=5.0).max_decimals(2));
                    ui.label(RichText::new("1 — równo w promieniu, 1,5 — MMSN").small().weak());
                });
            }
            ProfileKind::Formula => {
                ui.add(egui::TextEdit::singleline(&mut profile.formula).desired_width(f32::INFINITY).code_editor());
                let functions = function_names().collect::<Vec<_>>().join(", ");
                ui.label(
                    RichText::new(format!("Zmienna r [AU]; działania + − * / ^; stałe pi, e; funkcje: {functions}"))
                        .small()
                        .weak(),
                );
            }
            ProfileKind::Bezier => {
                ui.label(
                    RichText::new("Przeciągaj punkty ● i uchwyty ○ • dwuklik — nowy punkt • prawy klik — usuń punkt")
                        .small()
                        .weak(),
                );
            }
        }

        let table = profile.table(lo, hi);
        self.plot(ui, profile, table.as_deref().ok(), (lo, hi));
        if let Err(err) = &table {
            ui.colored_label(Color32::from_rgb(230, 110, 90), err);
        }

        if profile.kind == ProfileKind::Bezier {
            self.curve_controls(ui, &mut profile.curve, (lo, hi));
        } else if table.is_ok()
            && ui
                .button("✏ Edytuj jako krzywą")
                .on_hover_text("Zamienia ten profil na krzywą Béziera, którą można dalej kształtować myszą")
                .clicked()
        {
            profile.convert_to_curve(lo, hi, CONVERTED_POINTS).ok();
            self.selected = None;
        }
        *profile != before
    }

    fn plot(&mut self, ui: &mut egui::Ui, profile: &mut DensityProfile, table: Option<&[f64]>, (lo, hi): (f64, f64)) {
        let editable = profile.kind == ProfileKind::Bezier;
        let sense = if editable { Sense::click_and_drag() } else { Sense::hover() };
        let (response, painter) = ui.allocate_painter(vec2(ui.available_width(), PLOT_HEIGHT), sense);
        let rect = response.rect;
        let plot = Rect::from_min_max(rect.min + vec2(10.0, 18.0), rect.max - vec2(10.0, 18.0));
        #[cfg(test)]
        {
            self.plot_rect = Some(plot);
        }
        let to_screen = |p: Point| pos2(plot.left() + p.x as f32 * plot.width(), plot.bottom() - p.y as f32 * plot.height());
        let from_screen = |s: Pos2| {
            Point::new(((s.x - plot.left()) / plot.width()) as f64, ((plot.bottom() - s.y) / plot.height()) as f64)
        };

        // Tło, siatka i opisy osi.
        let visuals = ui.visuals();
        painter.rect_filled(rect, 4.0, visuals.extreme_bg_color);
        let grid = Stroke::new(1.0, visuals.weak_text_color().gamma_multiply(0.25));
        for q in 0..=4 {
            let f = q as f32 / 4.0;
            painter.hline(plot.x_range(), plot.bottom() - f * plot.height(), grid);
            painter.vline(plot.left() + f * plot.width(), plot.y_range(), grid);
        }
        let font = FontId::proportional(11.0);
        let weak = visuals.weak_text_color();
        painter.text(pos2(plot.left(), rect.bottom() - 3.0), Align2::LEFT_BOTTOM, format!("{lo:.2} AU"), font.clone(), weak);
        painter.text(pos2(plot.right(), rect.bottom() - 3.0), Align2::RIGHT_BOTTOM, format!("{hi:.2} AU"), font.clone(), weak);
        painter.text(pos2(plot.center().x, rect.bottom() - 3.0), Align2::CENTER_BOTTOM, "r", font.clone(), weak);
        painter.text(pos2(plot.left(), rect.top() + 3.0), Align2::LEFT_TOP, "Σ (względna)", font, weak);

        let line = Stroke::new(2.0, Color32::from_rgb(140, 180, 255));
        if !editable {
            if let Some(table) = table {
                let max = table.iter().copied().fold(0.0, f64::max);
                let pts = table
                    .iter()
                    .enumerate()
                    .map(|(k, &v)| to_screen(Point::new(k as f64 / TABLE_SIZE as f64, v / max)))
                    .collect();
                painter.add(Shape::line(pts, line));
            }
            return;
        }

        let curve = &mut profile.curve;
        if self.selected.is_some_and(|i| i >= curve.anchors.len()) {
            self.selected = None;
        }
        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let hit = pointer.and_then(|p| hit_test(curve, p, to_screen));

        if response.drag_started() {
            // Przeciąganie zaczyna się dopiero po ruchu myszy — liczy się to, co było pod kursorem
            // w chwili wciśnięcia przycisku.
            let origin = ui.input(|i| i.pointer.press_origin()).or(pointer);
            self.dragging = origin.and_then(|p| hit_test(curve, p, to_screen));
            if let Some(t) = self.dragging {
                self.selected = Some(t.anchor());
            }
        }
        if response.dragged()
            && let (Some(target), Some(p)) = (self.dragging, pointer)
        {
            let to = from_screen(p);
            match target {
                Target::Anchor(i) => curve.move_anchor(i, to),
                Target::Handle(i, side) => {
                    let a = curve.anchors[i].pos;
                    curve.move_handle(i, side, Point::new(to.x - a.x, to.y - a.y));
                }
            }
        }
        if response.drag_stopped() {
            self.dragging = None;
        }
        if response.double_clicked() {
            if hit.is_none()
                && let Some(p) = pointer
            {
                self.selected = curve.insert(from_screen(p)).or(self.selected);
            }
        } else if response.clicked() {
            self.selected = hit.map(Target::anchor);
        }
        if response.secondary_clicked()
            && let Some(Target::Anchor(i)) = hit
            && curve.remove(i)
        {
            self.selected = None;
        }
        if self.dragging.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if hit.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }

        // Krzywa, uchwyty i punkty.
        painter.add(Shape::line(curve.polyline(32).into_iter().map(to_screen).collect(), line));
        let handle_color = Color32::from_gray(170);
        for a in &curve.anchors {
            for h in [a.left, a.right] {
                if h != Point::ZERO {
                    let end = to_screen(Point::new(a.pos.x + h.x, a.pos.y + h.y));
                    painter.line_segment([to_screen(a.pos), end], Stroke::new(1.0, handle_color));
                    painter.circle_stroke(end, 3.5, Stroke::new(1.5, handle_color));
                }
            }
        }
        for (i, a) in curve.anchors.iter().enumerate() {
            let center = to_screen(a.pos);
            let color = if self.selected == Some(i) { Color32::from_rgb(255, 200, 80) } else { Color32::WHITE };
            if a.smooth {
                painter.circle_filled(center, 4.5, color);
            } else {
                // Punkt narożny (uchwyty niezależne) jako kwadrat.
                painter.rect_filled(Rect::from_center_size(center, vec2(8.0, 8.0)), 0.0, color);
            }
        }
    }

    fn curve_controls(&mut self, ui: &mut egui::Ui, curve: &mut BezierCurve, (lo, hi): (f64, f64)) {
        if let Some(i) = self.selected {
            let a = curve.anchors[i];
            let is_end = i == 0 || i + 1 == curve.anchors.len();
            ui.horizontal(|ui| {
                ui.label(format!("Punkt {}: r = {:.3} AU, Σ = {:.2}", i + 1, lo + a.pos.x * (hi - lo), a.pos.y));
                let mut smooth = a.smooth;
                if ui
                    .checkbox(&mut smooth, "gładki")
                    .on_hover_text("Gładki: uchwyty na jednej prostej, bez załamania krzywej.\nNarożny (■): uchwyty niezależne.")
                    .changed()
                {
                    if smooth {
                        curve.auto_handles(i);
                    } else {
                        curve.anchors[i].smooth = false;
                    }
                }
                if ui.add_enabled(!is_end, egui::Button::new("🗑")).on_hover_text("Usuń punkt").clicked() {
                    curve.remove(i);
                    self.selected = None;
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("➕ Dodaj punkt").on_hover_text("Nowy punkt w środku najdłuższego odcinka").clicked() {
                let seg = (0..curve.anchors.len() - 1)
                    .max_by(|&a, &b| {
                        let len = |s: usize| curve.anchors[s + 1].pos.x - curve.anchors[s].pos.x;
                        len(a).total_cmp(&len(b))
                    })
                    .unwrap_or(0);
                let x = 0.5 * (curve.anchors[seg].pos.x + curve.anchors[seg + 1].pos.x);
                self.selected = curve.insert(Point::new(x, curve.eval(x))).or(self.selected);
            }
            if ui.button("〰 Wygładź").on_hover_text("Automatyczne uchwyty we wszystkich punktach").clicked() {
                curve.smooth_all();
            }
            if ui.button("↺ Domyślna").clicked() {
                *curve = BezierCurve::default();
                self.selected = None;
            }
        });
    }
}

/// Najbliższy punkt lub uchwyt w zasięgu `HIT_RADIUS` od kursora (punkty mają lekkie pierwszeństwo).
fn hit_test(curve: &BezierCurve, pointer: Pos2, to_screen: impl Fn(Point) -> Pos2) -> Option<Target> {
    let mut best: Option<(f32, Target)> = None;
    let mut consider = |p: Point, bonus: f32, target: Target| {
        let d = to_screen(p).distance(pointer) - bonus;
        if d <= HIT_RADIUS && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, target));
        }
    };
    for (i, a) in curve.anchors.iter().enumerate() {
        consider(a.pos, 1.5, Target::Anchor(i));
        for (h, side) in [(a.left, Side::Left), (a.right, Side::Right)] {
            if h != Point::ZERO {
                consider(Point::new(a.pos.x + h.x, a.pos.y + h.y), 0.0, Target::Handle(i, side));
            }
        }
    }
    best.map(|(_, t)| t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, Modifiers, PointerButton, RawInput};

    /// Edytor uruchamiany bez okna: każde wywołanie `frame` to jedna klatka interfejsu.
    struct Harness {
        ctx: egui::Context,
        editor: ProfileEditor,
        profile: DensityProfile,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            let profile = DensityProfile { kind: ProfileKind::Bezier, ..Default::default() };
            let mut h = Self { ctx: egui::Context::default(), editor: ProfileEditor::default(), profile, time: 0.0 };
            h.frame(vec![]);
            h
        }

        fn frame(&mut self, events: Vec<Event>) -> bool {
            self.time += 0.05;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 500.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut changed = false;
            let (editor, profile) = (&mut self.editor, &mut self.profile);
            let _ = self.ctx.run_ui(input, |ui| changed |= editor.show(ui, profile, (1.0, 5.0)));
            changed
        }

        fn screen(&self, p: Point) -> Pos2 {
            let plot = self.editor.plot_rect.unwrap();
            pos2(plot.left() + p.x as f32 * plot.width(), plot.bottom() - p.y as f32 * plot.height())
        }

        fn button(&mut self, pos: Pos2, button: PointerButton, pressed: bool) -> bool {
            let event = Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(pos), event])
        }

        fn click(&mut self, pos: Pos2, button: PointerButton) -> bool {
            self.button(pos, button, true) | self.button(pos, button, false)
        }

        fn drag(&mut self, from: Pos2, to: Pos2) -> bool {
            let mut changed = self.button(from, PointerButton::Primary, true);
            for k in 1..=5 {
                changed |= self.frame(vec![Event::PointerMoved(from + (to - from) * k as f32 / 5.0)]);
            }
            changed | self.button(to, PointerButton::Primary, false)
        }
    }

    #[test]
    fn dragging_anchor_moves_it() {
        let mut h = Harness::new();
        let a = h.profile.curve.anchors[1].pos;
        let target = Point::new(0.5, 0.6);
        assert!(h.drag(h.screen(a), h.screen(target)));
        let moved = h.profile.curve.anchors[1].pos;
        assert!((moved.x - 0.5).abs() < 0.01 && (moved.y - 0.6).abs() < 0.01, "{moved:?}");
        assert_eq!(h.editor.selected, Some(1));
    }

    #[test]
    fn dragging_handle_rotates_both_handles_of_smooth_anchor() {
        let mut h = Harness::new();
        let a = h.profile.curve.anchors[1];
        let handle = Point::new(a.pos.x + a.right.x, a.pos.y + a.right.y);
        h.drag(h.screen(handle), h.screen(Point::new(handle.x, a.pos.y - 0.2)));
        let b = h.profile.curve.anchors[1];
        assert_eq!(b.pos, a.pos, "punkt nie powinien się ruszyć");
        assert!(b.right.y < -0.1, "uchwyt w dół: {:?}", b.right);
        assert!(b.left.y > 0.0, "gładki punkt: drugi uchwyt w górę: {:?}", b.left);
    }

    #[test]
    fn double_click_adds_and_right_click_removes_point() {
        let mut h = Harness::new();
        let n = h.profile.curve.anchors.len();
        let empty = h.screen(Point::new(0.75, 0.9));
        h.click(empty, PointerButton::Primary);
        h.click(empty, PointerButton::Primary);
        assert_eq!(h.profile.curve.anchors.len(), n + 1);
        let added = h.profile.curve.anchors.iter().position(|a| (a.pos.x - 0.75).abs() < 0.01).unwrap();
        assert!((h.profile.curve.anchors[added].pos.y - 0.9).abs() < 0.01);

        h.click(h.screen(h.profile.curve.anchors[added].pos), PointerButton::Secondary);
        assert_eq!(h.profile.curve.anchors.len(), n);
    }
}
