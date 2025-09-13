use crate::{config::Config, audio::{AudioSource, get_audio_sources, get_best_source_index}};
use eframe::egui;
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Instant,
    net::{UdpSocket, SocketAddr},
};
use tokio::runtime::Handle;

pub struct AudioStreamerApp {
    config: Config,
    config_path: PathBuf,
    sources: Arc<Mutex<Vec<AudioSource>>>,
    selected_source: usize,
    streaming: bool,
    ffmpeg_process: Option<Child>,
    status_message: String,
    last_refresh: Instant,
    runtime_handle: Handle,
    temp_ip: String,
    temp_port: String,
    network_test_result: String,
}

impl AudioStreamerApp {
    pub fn new(config: Config, config_path: PathBuf, runtime_handle: Handle) -> Self {
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
            last_refresh: Instant::now(),
            runtime_handle,
            temp_ip,
            temp_port,
            network_test_result: String::new(),
        };

        app.refresh_sources();
        app
    }

    fn refresh_sources(&self) {
        let sources_arc = Arc::clone(&self.sources);
        let runtime_handle = self.runtime_handle.clone();
        let config = self.config.clone();

        runtime_handle.spawn(async move {
            match get_audio_sources().await {
                Ok(new_sources) => {
                    if let Ok(mut sources) = sources_arc.lock() {
                        let best_index = if let Some(preferred) = &config.preferred_source {
                            // Check if preferred source still exists and prioritize it
                            new_sources.iter().position(|s| s.name == *preferred)
                                .unwrap_or_else(|| get_best_source_index(&new_sources))
                        } else {
                            get_best_source_index(&new_sources)
                        };

                        *sources = new_sources;
                        
                        // Note: We can't directly update selected_source here due to threading
                        // The UI will need to check and update it
                    }
                }
                Err(e) => {
                    eprintln!("Failed to refresh sources: {}", e);
                }
            }
        });
    }

    fn update_selected_source(&mut self) {
        let sources = self.sources.lock().unwrap();
        if sources.is_empty() {
            return;
        }

        let new_index = if let Some(preferred) = &self.config.preferred_source {
            sources.iter().position(|s| s.name == *preferred)
                .unwrap_or_else(|| get_best_source_index(&sources))
        } else {
            get_best_source_index(&sources)
        };

        if new_index != self.selected_source && new_index < sources.len() {
            self.selected_source = new_index;
            if let Some(source) = sources.get(new_index) {
                self.status_message = format!("Auto-selected: {}", source.description);
            }
        }
    }

    fn test_network_connectivity(&mut self) {
        if let (Ok(ip), Ok(port)) = (self.temp_ip.parse::<std::net::IpAddr>(), self.temp_port.parse::<u16>()) {
            match UdpSocket::bind("0.0.0.0:0") {
                Ok(socket) => {
                    let target = SocketAddr::new(ip, port);
                    match socket.send_to(b"audio-streamer-test", target) {
                        Ok(_) => {
                            self.network_test_result = "✅ Network test packet sent successfully".to_string();
                        }
                        Err(e) => {
                            self.network_test_result = format!("❌ Failed to send test packet: {}", e);
                        }
                    }
                }
                Err(e) => {
                    self.network_test_result = format!("❌ Failed to create UDP socket: {}", e);
                }
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
            
            // Add verbose logging for debugging
            let mut debug_args = vec!["-v".to_string(), "info".to_string()];
            debug_args.extend(args);
            
            let child = Command::new("ffmpeg")
                .args(&debug_args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?;

            self.ffmpeg_process = Some(child);
            self.streaming = true;
            self.status_message = format!(
                "Streaming {} to {}:{} (Check VLC: udp://@:{})",
                source.description,
                self.config.target_ip,
                self.config.target_port,
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
        }
    }

    fn generate_test_tone(&mut self) -> anyhow::Result<()> {
        if !self.config.is_ip_configured() {
            self.status_message = "Please set target IP first".to_string();
            return Ok(());
        }

        let target = format!("udp://{}:{}", self.config.target_ip, self.config.target_port);
        let args = vec![
            "-f".to_string(),
            "lavfi".to_string(),
            "-i".to_string(),
            "sine=frequency=440:duration=5".to_string(),
            "-c:a".to_string(),
            "aac".to_string(),
            "-f".to_string(),
            "mpegts".to_string(),
            target,
        ];

        let child = Command::new("ffmpeg")
            .args(&args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        self.status_message = "Sending 5-second test tone (440Hz)...".to_string();
        
        // Don't store this as the main process since it's temporary
        let runtime_handle = self.runtime_handle.clone();
        runtime_handle.spawn(async move {
            let mut child = child;
            let _ = child.wait();
        });

        Ok(())
    }

    fn format_source_display(&self, source: &AudioSource) -> String {
        let icon = if source.is_monitor { "🔊" } else { "🎤" };
        let status_indicators = format!(
            "{}{}{}",
            if source.is_running { " ⚡" } else { "" },
            if source.is_default { " ⭐" } else { "" },
            ""
        );
        format!("{} {}{}", icon, source.description, status_indicators)
    }
}

impl eframe::App for AudioStreamerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check if FFmpeg process is still running
        if self.streaming {
            if let Some(process) = &mut self.ffmpeg_process {
                if let Ok(Some(_)) = process.try_wait() {
                    self.streaming = false;
                    self.ffmpeg_process = None;
                    self.status_message = "Streaming stopped unexpectedly".to_string();
                }
            }
        }

        // Update selected source based on current state
        self.update_selected_source();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("🎵 Audio Streamer");
            ui.separator();

            // Configuration section
            egui::CollapsingHeader::new("⚙️ Configuration")
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new("config_grid")
                        .num_columns(2)
                        .spacing([10.0, 10.0])
                        .show(ui, |ui| {
                            ui.label("Target IP:");
                            ui.text_edit_singleline(&mut self.temp_ip);
                            ui.end_row();

                            ui.label("Target Port:");
                            ui.text_edit_singleline(&mut self.temp_port);
                            ui.end_row();

                            ui.label("VLC on phone:");
                            ui.colored_label(
                                egui::Color32::from_rgb(33, 150, 243),
                                format!("udp://@:{}", self.config.target_port)
                            );
                            ui.end_row();
                        });

                    ui.horizontal(|ui| {
                        if ui.button("Apply Settings").clicked() {
                            self.update_config_from_temp();
                            self.status_message = "Settings updated".to_string();
                        }
                        
                        if ui.button("💾 Save Config").clicked() {
                            self.update_config_from_temp();
                            if let Err(e) = self.save_config() {
                                self.status_message = format!("Save failed: {}", e);
                            }
                        }
                    });
                });

            ui.separator();

            // Network Testing Section
            egui::CollapsingHeader::new("🔧 Network Testing")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("🔍 Test Network Connection").clicked() {
                            self.test_network_connectivity();
                        }
                        
                        if ui.button("🎵 Send Test Tone (440Hz, 5sec)").clicked() {
                            if let Err(e) = self.generate_test_tone() {
                                self.network_test_result = format!("Test tone failed: {}", e);
                            }
                        }
                    });

                    if !self.network_test_result.is_empty() {
                        ui.separator();
                        ui.label(&self.network_test_result);
                    }

                    ui.separator();
                    ui.label("Troubleshooting tips:");
                    ui.label("• Make sure both devices are on the same WiFi network");
                    ui.label("• On phone: VLC → Open Network Stream → udp://@:1234");
                    ui.label("• Try the test tone first to verify connectivity");
                    ui.label("• Check if firewall is blocking UDP traffic");
                });

            ui.separator();

            // Audio source section
            egui::CollapsingHeader::new("🔊 Audio Source")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Select audio source:");
                        if ui.button("🔄 Refresh").clicked() {
                            self.refresh_sources();
                            self.status_message = "Refreshing sources...".to_string();
                        }
                    });

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Legend:");
                        ui.colored_label(egui::Color32::from_rgb(255, 193, 7), "⚡ = Active/Running");
                        ui.colored_label(egui::Color32::from_rgb(255, 193, 7), "⭐ = Default");
                    });

                    let sources = self.sources.lock().unwrap();
                    if sources.is_empty() {
                        ui.label("No audio sources found. Click Refresh to reload.");
                    } else {
                        // Clone the sources to avoid borrowing issues
                        let sources_clone = sources.clone();
                        drop(sources); // Explicitly drop the guard
                        
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                for (i, source) in sources_clone.iter().enumerate() {
                                    let text = self.format_source_display(source);
                                    
                                    let mut response = ui.selectable_label(
                                        i == self.selected_source,
                                        &text
                                    );

                                    // Highlight running sources
                                    if source.is_running {
                                        response = response.highlight();
                                    }
                                    
                                    if response.clicked() {
                                        self.selected_source = i;
                                        self.config.preferred_source = Some(source.name.clone());
                                        self.status_message = format!("Selected: {}", source.description);
                                    }
                                }
                            });
                    }
                });

            ui.separator();

            // Control buttons
            ui.horizontal(|ui| {
                let stream_button_text = if self.streaming { "⏹ Stop Streaming" } else { "▶ Start Streaming" };
                let stream_button_color = if self.streaming {
                    egui::Color32::from_rgb(244, 67, 54) // Red
                } else {
                    egui::Color32::from_rgb(76, 175, 80) // Green
                };

                let stream_button = egui::Button::new(stream_button_text)
                    .fill(stream_button_color)
                    .min_size(egui::vec2(120.0, 30.0));

                let enabled = self.config.is_ip_configured();
                if ui.add_enabled(enabled, stream_button).clicked() {
                    self.update_config_from_temp();
                    if self.streaming {
                        if let Err(e) = self.stop_streaming() {
                            self.status_message = format!("Stop failed: {}", e);
                        }
                    } else {
                        if let Err(e) = self.start_streaming() {
                            self.status_message = format!("Start failed: {}", e);
                        }
                    }
                }

                ui.separator();

                ui.label("Target:");
                let target_text = if self.config.target_ip.is_empty() {
                    "NOT SET".to_string()
                } else {
                    format!("{}:{}", self.config.target_ip, self.config.target_port)
                };
                let target_color = if self.config.is_ip_configured() {
                    egui::Color32::from_rgb(76, 175, 80) // Green
                } else {
                    egui::Color32::from_rgb(244, 67, 54) // Red
                };
                ui.colored_label(target_color, target_text);
            });

            ui.separator();

            // Status bar
            ui.horizontal(|ui| {
                ui.label("Status:");
                let status_color = if self.streaming {
                    egui::Color32::from_rgb(76, 175, 80) // Green
                } else if !self.config.is_ip_configured() {
                    egui::Color32::from_rgb(244, 67, 54) // Red
                } else {
                    egui::Color32::from_rgb(255, 152, 0) // Orange
                };
                ui.colored_label(status_color, &self.status_message);
            });
        });

        // Request repaint every 2 seconds to update streaming status and running sources
        ctx.request_repaint_after(std::time::Duration::from_secs(2));
    }
}