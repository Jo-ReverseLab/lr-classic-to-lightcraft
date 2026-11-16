//! The standalone lr2lc window.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![forbid(unsafe_code)]

use lr2lc_gui::app::App;
use lr2lc_gui::history::History;
use lr2lc_gui::host::StandaloneHost;
use lr2lc_gui::settings::Settings;
use lr2lc_gui::theme;

struct Window {
    app: App,
    host: StandaloneHost,
    settings_file: Option<std::path::PathBuf>,
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui, &mut self.host);
        if self.app.take_changed()
            && let Some(file) = &self.settings_file
        {
            // Settings are a convenience; a failed save only loses them.
            let _ = self.app.settings.save(file);
        }
    }
}

fn main() -> eframe::Result {
    let settings_file = Settings::file();
    let settings = settings_file
        .as_deref()
        .map(Settings::load)
        .unwrap_or_default();
    let history = History::default_dir().map(History::new);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("lr2lc")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([420.0, 480.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "lr2lc",
        options,
        Box::new(move |cc| {
            // Fonts and both themes are set before the first frame, so the
            // window never shows the wrong theme.
            theme::install_fonts(&cc.egui_ctx);
            theme::apply(&cc.egui_ctx, settings.theme);
            Ok(Box::new(Window {
                app: App::new(settings, history),
                host: StandaloneHost::default(),
                settings_file,
            }))
        }),
    )
}
