use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};
use jsonc_parser::ParseOptions;
use serde::Deserialize;

use floppier_proto::ParallelMode;

use crate::FloppierArgs;

#[derive(Deserialize, Debug)]
pub struct SongConfig {
    /// MIDI file to play and some play settings
    pub midi: MidiConfig,

    /// List of floppy drive configurations
    pub floppy_drives: Vec<FloppyDrive>,
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
pub struct FloppyDrive {
    pub id: u16,
    pub drive_count: u8,
    pub movement: bool,
    pub tracks: BTreeMap<u16, BTreeMap<u8, Vec<u8>>>,
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

    let config: SongConfig = serde_json::from_value(
        jsonc_parser::parse_to_serde_value(&config_file, &ParseOptions::default())
            .with_context(|| format!("could not parse file `{}`", args.path.display()))?
            .unwrap(),
    )
    .with_context(|| "configuration file format is invalid")?;

    Ok(config)
}
