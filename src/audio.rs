use anyhow::{Context, Result};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct AudioSource {
    pub name: String,
    pub description: String,
    pub is_monitor: bool,
    pub is_running: bool, // Now accurately reflects RUNNING vs IDLE/SUSPENDED
    pub is_default: bool, // Now accurately reflects the default SINK
}

// Fetches the name of the monitor for the default *output* device (speakers/headphones).
// This is what you actually want to stream to "hear what's playing".
async fn get_default_sink_monitor_name() -> Result<String> {
    let output = Command::new("pactl")
        .args(&["get-default-sink"])
        .output()
        .context("Failed to run 'pactl get-default-sink'")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!("Failed to get default sink"));
    }

    let sink_name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(format!("{}.monitor", sink_name))
}

// A robust parser for `pactl list sources` that handles the block-based output correctly.
// This ensures that the state (RUNNING, IDLE, SUSPENDED) is always correctly
// associated with its source name.
fn parse_pactl_sources_output(output: &str) -> Vec<(String, String, String)> {
    let mut sources = Vec::new();
    // Split the output into blocks for each source. Each block starts with "Source #".
    for block in output.split("Source #") {
        if block.trim().is_empty() {
            continue;
        }

        let mut name: Option<String> = None;
        let mut description: Option<String> = None;
        let mut state: Option<String> = None;

        for line in block.lines() {
            let trimmed = line.trim();
            if let Some(val) = trimmed.strip_prefix("Name:") {
                name = Some(val.trim().to_string());
            } else if let Some(val) = trimmed.strip_prefix("Description:") {
                description = Some(val.trim().to_string());
            } else if let Some(val) = trimmed.strip_prefix("State:") {
                state = Some(val.trim().to_string());
            }
        }

        if let (Some(name), Some(description), Some(state)) = (name, description, state) {
            sources.push((name, description, state));
        }
    }
    sources
}


pub async fn get_audio_sources() -> Result<Vec<AudioSource>> {
    let sources_list_output = Command::new("pactl")
        .args(&["list", "sources"])
        .output()
        .context("Failed to run 'pactl list sources'")?;

    if !sources_list_output.status.success() {
        return Err(anyhow::anyhow!("Failed to list sources"));
    }
    let sources_stdout = String::from_utf8_lossy(&sources_list_output.stdout);

    // Get the accurate information using the new robust functions
    let parsed_sources = parse_pactl_sources_output(&sources_stdout);
    let default_sink_monitor = get_default_sink_monitor_name().await.unwrap_or_default();

    let mut sources: Vec<AudioSource> = parsed_sources.into_iter()
        .map(|(name, description, state)| {
            let is_monitor = name.contains(".monitor");
            // THIS IS THE CRITICAL FIX: Only a state of "RUNNING" counts.
            // "IDLE" and "SUSPENDED" will correctly be treated as not running.
            let is_running = state == "RUNNING";
            let is_default = name == default_sink_monitor;

            AudioSource { name, description, is_monitor, is_running, is_default }
        })
        .collect();

    // Sort sources using a scoring system. A running default is top priority.
    sources.sort_by(|a, b| {
        // Higher score is better. A running device gets a huge boost.
        let score = |s: &AudioSource| -> i32 {
            let mut score = 0;
            if s.is_running { score += 4; } // Actively playing audio is most important
            if s.is_default { score += 2; } // Being the default sink is next most important
            if s.is_monitor { score += 1; } // Monitors are preferred over mics
            score
        };
        // Sort descending by score, then alphabetically by description as a tie-breaker.
        score(b).cmp(&score(a)).then_with(|| a.description.cmp(&b.description))
    });

    Ok(sources)
}

pub fn get_best_source_index(sources: &[AudioSource]) -> usize {
    // Because the list is now sorted with the highest-priority device at the top,
    // the best source is always the first one.
    0
}