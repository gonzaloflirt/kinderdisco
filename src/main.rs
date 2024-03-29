mod app;

use crate::app::App;
use app::SyncMode;

fn main() {
    let native_options = eframe::NativeOptions {
        initial_window_size: Some([400.0, 340.0].into()),
        min_window_size: Some([300.0, 200.0].into()),
        follow_system_theme: true,
        ..Default::default()
    };
    let _ = eframe::run_native(
        "kinderdisco",
        native_options,
        Box::new(|cc| {
            let mut app = Box::new(App::new(cc));
            app.get_bridge_ip();
            app
        }),
    );
}

fn update_start<T>(range: &mut core::ops::Range<T>)
where
    T: Ord + Copy,
{
    range.start = std::cmp::min(range.start, range.end)
}

fn update_end<T>(range: &mut core::ops::Range<T>)
where
    T: Ord + Copy,
{
    range.end = std::cmp::max(range.start, range.end)
}

fn label(sync_mode: SyncMode) -> &'static str {
    match sync_mode {
        SyncMode::None => "none",
        SyncMode::Time => "time",
        SyncMode::TimeAndColor => "time & color",
    }
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }

    fn draw_not_connected(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::Label::new("No Hue Bridge found."));
            if ui.add(egui::Button::new("Search Bridge")).clicked() {
                self.get_bridge_ip();
            }
            if let Some(error) = &self.error {
                ui.add(egui::Label::new(error));
            }
        });
    }

    fn draw_register(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add(egui::Label::new(
                "Press the button on the bridge to register.",
            ));
            if ui.add(egui::Button::new("Register user")).clicked() {
                self.register_user(self.ip.unwrap());
            }
            if let Some(error) = &self.error {
                ui.add(egui::Label::new(error));
            }
        });
    }

    fn draw_connected(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            if ui
                .add(egui::Slider::new(&mut self.data.r.start, 0..=255).text("r min"))
                .changed()
            {
                update_end(&mut self.data.r)
            }
            if ui
                .add(egui::Slider::new(&mut self.data.r.end, 0..=255).text("r max"))
                .changed()
            {
                update_start(&mut self.data.r)
            }

            if ui
                .add(egui::Slider::new(&mut self.data.g.start, 0..=255).text("g min"))
                .changed()
            {
                update_end(&mut self.data.g)
            }
            if ui
                .add(egui::Slider::new(&mut self.data.g.end, 0..=255).text("g max"))
                .changed()
            {
                update_start(&mut self.data.g)
            }

            if ui
                .add(egui::Slider::new(&mut self.data.b.start, 0..=255).text("b min"))
                .changed()
            {
                update_end(&mut self.data.b)
            }
            if ui
                .add(egui::Slider::new(&mut self.data.b.end, 0..=255).text("b max"))
                .changed()
            {
                update_start(&mut self.data.b)
            }
            ui.add(egui::Separator::default());
            if ui
                .add(egui::Slider::new(&mut self.data.time.start, 1..=100).text("time (100ms) min"))
                .changed()
            {
                update_end(&mut self.data.time)
            }
            if ui
                .add(egui::Slider::new(&mut self.data.time.end, 1..=100).text("time (100ms) max"))
                .changed()
            {
                update_start(&mut self.data.time)
            }
            ui.add(egui::Checkbox::new(&mut self.data.fade, "fade"));

            egui::ComboBox::from_label("sync mode")
                .selected_text(label(self.sync_mode))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.sync_mode, SyncMode::None, label(SyncMode::None));
                    ui.selectable_value(&mut self.sync_mode, SyncMode::Time, label(SyncMode::Time));
                    ui.selectable_value(
                        &mut self.sync_mode,
                        SyncMode::TimeAndColor,
                        label(SyncMode::TimeAndColor),
                    );
                });

            ui.add(egui::Separator::default());

            egui::ScrollArea::vertical().show(ui, |ui| {
                for (_, light) in &mut self.lights {
                    if ui
                        .add(egui::Checkbox::new(&mut light.on, light.light.name.clone()))
                        .changed()
                    {
                        self.rebuild_modulators = true;
                    }
                }

                ui.allocate_space(ui.available_size());
            });
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll();

        if self.bridge.is_some() {
            self.draw_connected(ctx, frame);
        } else if self.ip.is_some() {
            self.draw_register(ctx, frame);
        } else {
            self.draw_not_connected(ctx, frame);
        }

        self.update_data();
    }
}
