use anyhow::{Context, Result};
use clap::{Arg, Command};
use eframe::egui;
use std::{path::PathBuf, fs};

mod config;
mod audio;
mod gui;

use config::Config;
use gui::AudioStreamerApp;

#[tokio::main]
async fn main() -> Result<()> {
    // clap argument parsing remains the same...
    let matches = Command::new("Audio Streamer")
        .version("0.1.0")
        .about("Stream system audio to phone via UDP")
        .arg(
            Arg::new("config")
                .short('c')
                .long("config")
                .value_name("FILE")
                .help("Use custom config file")
        )
        .get_matches();

    let config_path = if let Some(config_file) = matches.get_one::<String>("config") {
        PathBuf::from(config_file)
    } else {
        get_default_config_path()?
    };

    let config = load_or_create_config(&config_path).await?;

    // --- KEY CHANGE: Set up a transparent, borderless window ---
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([600.0, 550.0]) // Increased height for better padding
            .with_min_inner_size([500.0, 450.0])
            .with_decorations(false) // No OS title bar, borders, etc.
            .with_transparent(true), // Enable transparency
        ..Default::default()
    };
    // --- END OF KEY CHANGE ---

    let rt = tokio::runtime::Handle::current();

    eframe::run_native(
        "Audio Streamer",
        options,
        // Pass the creation context to the app so we can apply styles
        Box::new(move |cc| {
            let app = AudioStreamerApp::new(config, config_path, rt, cc);
            Box::new(app)
        }),
    ).map_err(|e| anyhow::anyhow!("Failed to run GUI: {}", e))?;

    Ok(())
}

fn get_default_config_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir()
        .context("Could not find config directory")?
        .join("audio-streamer");
    
    std::fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("config.json"))
}

async fn load_or_create_config(path: &PathBuf) -> Result<Config> {
    if path.exists() {
        let content = fs::read_to_string(path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    } else {
        let config = Config::default();
        let json = serde_json::to_string_pretty(&config)?;
        fs::write(path, json)?;
        Ok(config)
    }
}