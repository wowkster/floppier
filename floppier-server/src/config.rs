use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

use floppier_proto::ParallelMode;

use crate::FloppierArgs;

#[derive(Deserialize, Debug)]
pub struct SongConfig {
    /// MIDI file to play and some play settings
    pub midi: MidiConfig,

    /// Tracks to play
    pub tracks: Vec<TrackConfig>,
}

#[derive(Deserialize, Debug)]
pub struct MidiConfig {
    /// Path to the MIDI file to play
    pub path: PathBuf,

    /// Strategy to use to resolve parallel notes
    #[serde(default)]
    pub parallel_mode: ParallelMode,
}

#[derive(Deserialize, Debug)]
pub struct TrackConfig {
    pub track: u16,
    pub channels: Vec<ChannelConfig>,
}

#[derive(Deserialize, Debug)]
pub struct ChannelConfig {
    pub channel: u8,
    pub ports: Vec<u8>,
}

pub fn parse_song_config(args: &FloppierArgs) -> Result<SongConfig> {
    if !args.path.exists() {
        return Err(anyhow::anyhow!(
            "song configuration file `{}` does not exist",
            args.path.display()
        ));
    }

    let config_file = std::fs::read_to_string(&args.path)
        .with_context(|| format!("could not read file `{}`", args.path.display()))?;

    let config: SongConfig = toml::from_str(&config_file)
        .with_context(|| format!("could not parse file `{}`", args.path.display()))?;

    Ok(config)
}
