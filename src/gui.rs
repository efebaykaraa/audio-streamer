use crate::{config::Config, audio::{AudioSource, get_audio_sources, get_best_source_index}};
use eframe::{egui, CreationContext};
use egui::{Color32, Stroke};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    net::{UdpSocket, SocketAddr},
};
use tokio::runtime::Handle;

// A function to set up our custom style.
fn configure_styles(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let visuals = &mut style.visuals;

    // --- COLOR PALETTE (Dark theme with purple/blue accents) ---
    let accent_color = Color32::from_rgb(110, 100, 255); // A nice purple
    let text_color = Color32::from_gray(230);
    let faint_color = Color32::from_gray(120);

    visuals.window_rounding = 10.0.into();
    visuals.window_shadow = egui::epaint::Shadow::big_dark();
    visuals.override_text_color = Some(text_color);
    visuals.extreme_bg_color = Color32::from_rgba_unmultiplied(10, 10, 15, 255); // For text edit, etc.

    // Widget styling
    let widget_visuals = &mut visuals.widgets;
    widget_visuals.noninteractive.bg_fill = Color32::from_gray(27);
    widget_visuals.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(200));
    widget_visuals.noninteractive.fg_stroke = Stroke::new(1.0, faint_color);
    widget_visuals.noninteractive.rounding = 5.0.into();

    widget_visuals.inactive = widget_visuals.noninteractive;
    widget_visuals.inactive.bg_fill = Color32::from_white_alpha(10); // Subtle background for buttons

    widget_visuals.hovered.bg_fill = Color32::from_white_alpha(20);
    widget_visuals.hovered.bg_stroke = Stroke::new(1.0, accent_color);

    widget_visuals.active.bg_fill = accent_color;
    widget_visuals.active.bg_stroke = Stroke::new(1.0, accent_color);
    widget_visuals.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);

    // Spacing and padding
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.window_margin = egui::Margin::same(15.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);

    ctx.set_style(style);
}

pub struct AudioStreamerApp {
    config: Config,
    config_path: PathBuf,
    sources: Arc<Mutex<Vec<AudioSource>>>,
    selected_source: usize,
    streaming: bool,
    ffmpeg_process: Option<Child>,
    status_message: String,
    runtime_handle: Handle,
    temp_ip: String,
    temp_port: String,
    network_test_result: String,
}

impl AudioStreamerApp {
    pub fn new(config: Config, config_path: PathBuf, runtime_handle: Handle, cc: &CreationContext) -> Self {
        // Apply the custom style on creation
        configure_styles(&cc.egui_ctx);
        
        let temp_ip = config.target_ip.clone();
        let temp_port = config.target_port.to_string();
        let status_message = if config.is_ip_configured() {
            "Ready to stream".to_string()
        } else {
            "Please set target IP address".to_string()
        };

        let mut app = Self {
            config,
            config_path,
            sources: Arc::new(Mutex::new(Vec::new())),
            selected_source: 0,
            streaming: false,
            ffmpeg_process: None,
            status_message,
            runtime_handle,
            temp_ip,
            temp_port,
            network_test_result: String::new(),
        };

        app.refresh_sources();
        app
    }

    // --- LOGIC METHODS (Unchanged from previous version) ---

    fn refresh_sources(&self) {
        let sources_arc = Arc::clone(&self.sources);
        let runtime_handle = self.runtime_handle.clone();

        runtime_handle.spawn(async move {
            match get_audio_sources().await {
                Ok(new_sources) => {
                    if let Ok(mut sources) = sources_arc.lock() {
                        *sources = new_sources;
                    }
                }
                Err(e) => {
                    eprintln!("Failed to refresh sources: {}", e);
                }
            }
        });
    }

    fn update_selected_source(&mut self) {
        let mut sources = self.sources.lock().unwrap();
        if sources.is_empty() {
            return;
        }

        let new_index = get_best_source_index(&sources);

        if new_index != self.selected_source && new_index < sources.len() {
            self.selected_source = new_index;
            if let Some(source) = sources.get(new_index) {
                if !self.streaming { // Only update status if not actively streaming
                    self.status_message = format!("Auto-selected: {}", source.description);
                }
            }
        }
    }

    fn test_network_connectivity(&mut self) {
        if let (Ok(ip), Ok(port)) = (self.temp_ip.parse::<std::net::IpAddr>(), self.temp_port.parse::<u16>()) {
            match UdpSocket::bind("0.0.0.0:0") {
                Ok(socket) => {
                    let target = SocketAddr::new(ip, port);
                    match socket.send_to(b"audio-streamer-test", target) {
                        Ok(_) => self.network_test_result = "✅ Network test packet sent successfully".to_string(),
                        Err(e) => self.network_test_result = format!("❌ Failed to send test packet: {}", e),
                    }
                }
                Err(e) => self.network_test_result = format!("❌ Failed to create UDP socket: {}", e),
            }
        } else {
            self.network_test_result = "❌ Invalid IP or port format".to_string();
        }
    }

    fn start_streaming(&mut self) -> anyhow::Result<()> {
        if !self.config.is_ip_configured() {
            self.status_message = "Please set target IP first".to_string();
            return Ok(());
        }

        let sources = self.sources.lock().unwrap();
        if let Some(source) = sources.get(self.selected_source) {
            let args = self.config.build_ffmpeg_command(&source.name);
            
            let child = Command::new("ffmpeg")
                .args(&args)
                .stdout(Stdio::null()) // Keep these null to avoid blocking
                .stderr(Stdio::null())
                .spawn()?;

            self.ffmpeg_process = Some(child);
            self.streaming = true;
            self.status_message = format!(
                "Streaming {} to {}:{}",
                source.description,
                self.config.target_ip,
                self.config.target_port
            );
        }
        Ok(())
    }

    fn stop_streaming(&mut self) -> anyhow::Result<()> {
        if let Some(mut process) = self.ffmpeg_process.take() {
            process.kill()?;
            process.wait()?;
        }
        self.streaming = false;
        self.status_message = "Streaming stopped".to_string();
        Ok(())
    }

    fn save_config(&mut self) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&self.config)?;
        fs::write(&self.config_path, json)?;
        self.status_message = "Configuration saved".to_string();
        Ok(())
    }

    fn update_config_from_temp(&mut self) {
        if !self.temp_ip.is_empty() && self.temp_ip != self.config.target_ip {
            self.config.target_ip = self.temp_ip.clone();
        }
        
        if let Ok(port) = self.temp_port.parse::<u16>() {
            if port != self.config.target_port {
                self.config.target_port = port;
            }
        } else {
             self.temp_port = self.config.target_port.to_string(); // Revert if invalid
        }
    }

    fn generate_test_tone(&mut self) -> anyhow::Result<()> {
        if !self.config.is_ip_configured() {
            self.status_message = "Please set target IP first".to_string();
            return Ok(());
        }
        let target = format!("udp://{}:{}", self.config.target_ip, self.config.target_port);
        let args = vec!["-f", "lavfi", "-i", "sine=frequency=440:duration=5", "-c:a", "aac", "-f", "mpegts", &target];
        Command::new("ffmpeg").args(&args).spawn()?;
        self.status_message = "Sending 5-second test tone (440Hz)...".to_string();
        Ok(())
    }

    fn format_source_display(&self, source: &AudioSource) -> String {
        let icon = if source.is_monitor { "🔊" } else { "🎤" };
        let status_indicators = format!(
            "{}{}",
            if source.is_running { " ⚡" } else { "" },
            if source.is_default { " ⭐" } else { "" },
        );
        format!("{} {}{}", icon, source.description, status_indicators)
    }
}

// --- APP DRAWING LOGIC ---

impl eframe::App for AudioStreamerApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // --- Process background logic ---
        if self.streaming {
            if let Some(process) = &mut self.ffmpeg_process {
                if process.try_wait().ok().flatten().is_some() {
                    self.streaming = false;
                    self.ffmpeg_process = None;
                    self.status_message = "Streaming stopped unexpectedly".to_string();
                }
            }
        }
        self.update_selected_source();
        
        let main_frame = egui::Frame {
            fill: Color32::from_rgba_unmultiplied(30, 30, 45, 255),
            inner_margin: egui::Margin::same(0.0),
            outer_margin: egui::Margin::same(0.0),
            rounding: 10.0.into(),
            stroke: Stroke::new(1.0, Color32::WHITE),
            ..Default::default()
        };

        egui::CentralPanel::default().frame(main_frame).show(ctx, |ui| {
            let app_rect = ui.max_rect();

            // --- Custom Title Bar ---
            let title_bar_height = 32.0;
            let title_bar_rect = egui::Rect::from_min_size(app_rect.min, egui::vec2(app_rect.width(), title_bar_height));
            let title_bar_response = ui.interact(title_bar_rect, egui::Id::new("title_bar"), egui::Sense::click_and_drag());
            
            if title_bar_response.dragged() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            // Draw title bar content
            let painter = ui.painter();
            painter.rect_filled(title_bar_rect, 5.0, Color32::from_rgb(50, 50, 65));
            
            ui.allocate_ui_at_rect(title_bar_rect, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("🎵 Audio Streamer").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(egui::RichText::new("❌").color(Color32::LIGHT_RED)).on_hover_text("Close").clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Close); }
                        if ui.button(egui::RichText::new("🗗").strong()).on_hover_text("Maximize").clicked() { 
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!ctx.input(|i| i.viewport().maximized.unwrap_or(false))));
                        }
                        if ui.button(egui::RichText::new("🗕").strong()).on_hover_text("Minimize").clicked() { ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)); }
                    });
                });
            });

            // --- Main Content Area ---
            let content_rect = egui::Rect::from_min_size(egui::pos2(app_rect.min.x, app_rect.min.y + title_bar_height), egui::vec2(app_rect.width(), app_rect.height() - title_bar_height));
            ui.allocate_ui_at_rect(content_rect, |ui| {
                ui.scope(|ui| {
                    ui.style_mut().spacing.window_margin = egui::Margin::same(15.0); // Apply padding for content
                    
                    // --- Configuration section ---
                    ui.collapsing(egui::RichText::new("⚙ Configuration").size(16.0), |ui| {
                        egui::Grid::new("config_grid").num_columns(2).spacing([10.0, 10.0]).show(ui, |ui| {
                            ui.label("Target IP:");
                            ui.text_edit_singleline(&mut self.temp_ip);
                            ui.end_row();
                            ui.label("Target Port:");
                            ui.text_edit_singleline(&mut self.temp_port);
                            ui.end_row();
                        });
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            if ui.button("Apply Settings").clicked() { self.update_config_from_temp(); self.status_message = "Settings updated".to_string(); }
                            if ui.button("💾 Save").clicked() { self.update_config_from_temp(); if let Err(e) = self.save_config() { self.status_message = format!("Save failed: {}", e); } }
                        });
                    });

                    // --- Audio Source Section ---
                    ui.vertical_centered(|ui| ui.collapsing(egui::RichText::new("🔊 Audio Source").size(16.0), |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Select audio source:");
                            if ui.button("🔄 Refresh").clicked() { self.refresh_sources(); self.status_message = "Refreshing...".to_string(); }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Legend:");
                            ui.colored_label(ui.visuals().widgets.active.bg_fill, "⚡=Active");
                            ui.colored_label(ui.visuals().widgets.active.bg_fill, "⭐=Default");
                        });

                        let sources = self.sources.lock().unwrap().clone();
                        let current_selected = self.selected_source;
                        egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                            for (i, source) in sources.iter().enumerate() {
                                let text = self.format_source_display(source);
                                let response = ui.selectable_label(i == current_selected, text);
                                if response.clicked() { self.selected_source = i; self.config.preferred_source = Some(source.name.clone()); self.status_message = format!("Selected: {}", source.description); }
                            }
                        });
                    }));

                    // --- Control & Status ---
                    egui::Frame {
                        inner_margin: egui::Margin::same(8.0),
                        outer_margin: egui::Margin::same(0.0),
                        rounding: ui.style().visuals.widgets.noninteractive.rounding,
                        stroke: Stroke::NONE,
                        ..Default::default()
                    }.show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            let stream_button_text = if self.streaming { "⏹ Stop Streaming" } else { "▶ Start Streaming" };
                            let stream_button_color = if self.streaming { Color32::from_rgb(200, 70, 70) } else { Color32::from_rgb(70, 170, 70) };
                            let stream_button = egui::Button::new(stream_button_text).fill(stream_button_color).min_size(egui::vec2(200.0, 40.0));
                            
                            if ui.add_enabled(self.config.is_ip_configured(), stream_button).clicked() {
                                self.update_config_from_temp();
                                if self.streaming { if let Err(e) = self.stop_streaming() { self.status_message = format!("Stop failed: {}", e); }}
                                else { if let Err(e) = self.start_streaming() { self.status_message = format!("Start failed: {}", e); }}
                            }

                            ui.separator();
                            let status_color = if self.streaming { Color32::from_rgb(76, 175, 80) } else if !self.config.is_ip_configured() { Color32::from_rgb(244, 67, 54) } else { Color32::from_rgb(255, 152, 0) };
                            ui.label(egui::RichText::new(format!("Status: {}", self.status_message)).color(status_color));
                        });
                    });

                }); // End of content scope
            }); // End of content area allocation
        });

        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}