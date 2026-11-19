//! `cargo run -p echo_egui --example gallery`

macro_rules! __echo_deny {
    ($($t:tt)*) => {};
}
pub(crate) use __echo_deny;

mod demo;

fn main() -> eframe::Result {
    let app = echo_egui::EguiApp::new();
    let mut state = demo::Demo::default();
    eframe::run_ui_native(
        "echo + egui",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([520.0, 640.0]),
            ..Default::default()
        },
        move |ui, _frame| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.show(ui, |b| demo::demo(b.upcast(), &mut state, &mut |_, _| {}));
            });
        },
    )
}
