//! Animacja 3D układu planetarnego z symulacją grawitacji N ciał.

// Na Windowsie w wersji release nie otwieraj dodatkowego okna konsoli.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod engine;
mod gas;
mod logger;
mod physics;
mod render;
mod system;
mod vec3;
mod video;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Układ planetarny 3D")
            .with_app_id("planet-animation")
            .with_inner_size([1400.0, 860.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Układ planetarny 3D",
        options,
        Box::new(|cc| {
            add_symbol_font(&cc.egui_ctx);
            Ok(Box::new(app::PlanetApp::new()))
        }),
    )
}

/// Wbudowane czcionki egui nie mają symboli astronomicznych (☉, ⊕, ♃…), więc dokładamy
/// podzbiór DejaVu Sans jako czcionkę zapasową (licencja w `assets/LICENSE-DejaVu.txt`).
fn add_symbol_font(ctx: &eframe::egui::Context) {
    use eframe::egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("symbole".into(), FontData::from_static(include_bytes!("../assets/symbole.ttf")).into());
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("symbole".into());
    }
    ctx.set_fonts(fonts);
}

/// `atan2f` z bieżącej glibc ma wersję GLIBC_2.43, przez co program nie uruchomiłby się na starszych
/// dystrybucjach. Własna definicja przez `atan2` z f64 (dostępne od zawsze) usuwa to wymaganie.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[unsafe(no_mangle)]
extern "C" fn atan2f(y: f32, x: f32) -> f32 {
    (y as f64).atan2(x as f64) as f32
}
