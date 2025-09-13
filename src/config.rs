use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub target_ip: String,
    pub target_port: u16,
    pub audio_codec: String,
    pub bitrate: String,
    pub sample_rate: u32,
    pub channels: u8,
    pub buffer_size: u32,
    pub low_latency: bool,
    pub preferred_source: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            target_ip: String::new(), // Empty by default, will prompt user
            target_port: 1234,
            audio_codec: "aac".to_string(),
            bitrate: "192k".to_string(),
            sample_rate: 48000,
            channels: 2,
            buffer_size: 1316,
            low_latency: true,
            preferred_source: None,
        }
    }
}

impl Config {
    pub fn is_ip_configured(&self) -> bool {
        !self.target_ip.is_empty() && self.target_ip != "0.0.0.0"
    }

    pub fn build_ffmpeg_command(&self, source: &str) -> Vec<String> {
        let mut cmd = vec![
            "-f".to_string(),
            "pulse".to_string(),
            "-i".to_string(),
            source.to_string(),
            "-ac".to_string(),
            self.channels.to_string(),
            "-ar".to_string(),
            self.sample_rate.to_string(),
            "-c:a".to_string(),
            self.audio_codec.clone(),
            "-b:a".to_string(),
            self.bitrate.clone(),
        ];

        if self.low_latency {
            cmd.extend([
                "-flags".to_string(),
                "+low_delay".to_string(),
                "-fflags".to_string(),
                "+nobuffer".to_string(),
                "-flush_packets".to_string(),
                "1".to_string(),
            ]);
        }

        cmd.extend([
            "-f".to_string(),
            "mpegts".to_string(),
            "-muxdelay".to_string(),
            "0".to_string(),
            "-muxpreload".to_string(),
            "0".to_string(),
            format!("udp://{}:{}?pkt_size={}", 
                   self.target_ip, self.target_port, self.buffer_size),
        ]);

        println!("FFmpeg command: ffmpeg {}", cmd.join(" "));

        cmd
    }
}