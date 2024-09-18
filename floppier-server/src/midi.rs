use std::{fmt::Display, path::Path};

use anyhow::{bail, ensure, Context, Result};
use midly::{Format, MetaMessage, MidiMessage, Smf, Timing, Track, TrackEvent, TrackEventKind};

use floppier_proto::LimitedMidiMessage;

#[derive(Debug)]
pub struct AbsoluteMidiEvent {
    pub time_offset: u32,
    pub track: u16,
    pub channel: u8,
    pub message: LimitedMidiMessage,
}

pub struct MidiFile {
    pub metadata: MidiMetadata,
    pub ticks_per_beat: u16,
    pub beats_per_minute: f64,
    pub num_tracks: u16,
    pub events: Vec<AbsoluteMidiEvent>,
}

pub fn parse_midi_file<P: AsRef<Path>>(midi_path: P) -> Result<MidiFile> {
    let midi_file = std::fs::read(midi_path)?;
    let smf = Smf::parse(&midi_file)?;

    /* Get Header Data */

    dbg!(smf.header);

    let (Format::Parallel | Format::SingleTrack) = smf.header.format else {
        bail!("only parallel format is supported");
    };

    let Timing::Metrical(ticks_per_beat) = smf.header.timing else {
        bail!("only metrical timing is supported");
    };

    /* Parse Metadata Track */

    let meta_track = smf
        .tracks
        .first()
        .with_context(|| "could not get first track")?;

    let (first_non_meta_index, metadata) = parse_track_metadata(meta_track)?;

    /* Calculate Tempo Values */

    let ticks_per_beat = ticks_per_beat.as_int();
    let beats_per_minute = tempo_to_bpm(metadata.tempo);

    /* Absolutize the time for each track */

    let data_tracks = match smf.header.format {
        // Single track with metadata at the beginning
        Format::SingleTrack => {
            vec![absolutize_track(
                &meta_track[first_non_meta_index..].to_vec(),
                1,
            )]
        }
        // Single metadata track + data tracks
        Format::Parallel => smf.tracks[1..]
            .iter()
            .enumerate()
            .map(|(i, track)| absolutize_track(track, (i + 1) as u16))
            .collect::<Vec<_>>(),
        Format::Sequential => unimplemented!(),
    };

    let num_tracks = data_tracks.len() as u16;

    ensure!(!data_tracks.is_empty(), "no data tracks found in MIDI file");

    // assert!(
    //     data_tracks.len() <= 2,
    //     "no more than 2 data tracks are supported"
    // );

    /* Combine the data tracks into a single list of events */

    let mut events = Vec::with_capacity(data_tracks.iter().map(|t| t.len()).sum());

    for track in data_tracks {
        events.extend(track);
    }

    events.sort_by_key(|e| e.time_offset);

    // for event in &events {
    //     println!("{:?}", event);
    // }

    Ok(MidiFile {
        metadata,
        ticks_per_beat,
        beats_per_minute,
        num_tracks,
        events,
    })
}

/// Takes a tempo in microseconds per beat and returns the tempo in beats per minute
pub fn tempo_to_bpm(tempo: u32) -> f64 {
    let beats_per_microsecond = 1.0 / tempo as f64;
    let beats_per_second = beats_per_microsecond * 1_000_000.0;

    beats_per_second * 60.0
}

/// Takes a number of ticks and returns the number of microseconds that many ticks represents
pub fn ticks_to_microseconds(ticks: u32, ticks_per_beat: u16, beats_per_minute: f64) -> u64 {
    let beats = ticks as f64 / ticks_per_beat as f64;
    let seconds = beats / beats_per_minute * 60.0;
    let microseconds = seconds * 1_000_000.0;

    microseconds as u64
}

#[derive(Debug)]
pub struct MidiMetadata {
    track_name: Option<String>,
    text: Vec<String>,
    copyright: Vec<String>,
    tempo: u32,
    time_signature: (u8, u8, u8, u8),
    key_signature: (i8, bool),
}

impl Display for MidiMetadata {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(track_name) = &self.track_name {
            writeln!(f, "Track Name: {}", track_name)?;
        }

        for txt in &self.text {
            writeln!(f, "Text: {}", txt)?;
        }

        for txt in &self.copyright {
            writeln!(f, "Copyright: {}", txt)?;
        }

        writeln!(f, "Tempo: {} bpm", tempo_to_bpm(self.tempo))?;
        writeln!(
            f,
            "Time Signature: {}/{} ({} clocks per tick, {} 32nd notes per beat)",
            self.time_signature.0,
            2u32.pow(self.time_signature.1 as u32),
            self.time_signature.2,
            self.time_signature.3
        )?;
        write!(
            f,
            "Key Signature: {} {} {}",
            self.key_signature.0.abs(),
            if self.key_signature.0 < 0 {
                "flat(s)"
            } else {
                "sharp(s)"
            },
            if self.key_signature.1 {
                "minor"
            } else {
                "major"
            }
        )?;

        Ok(())
    }
}

/// Parses the metadata from the given track and returns the index of the first
/// non-metadata event as well as the parsed metadata
fn parse_track_metadata(track: &Track) -> Result<(usize, MidiMetadata)> {
    let mut track_name = None;
    let mut text = Vec::new();
    let mut copyright = Vec::new();
    let mut tempo = None;
    let mut time_signature = None;
    let mut key_signature = None;

    assert!(!track.is_empty());

    let mut next_index = 0;

    for (i, TrackEvent { delta, kind }) in track.iter().enumerate() {
        assert_eq!(
            delta.as_int(),
            0,
            "metadata track should have no delta time"
        );

        dbg!(kind);

        let TrackEventKind::Meta(msg) = kind else {
            next_index = i;
            break;
        };

        match msg {
            MetaMessage::TrackName(name) => {
                assert_eq!(track_name, None, "only one track name is supported");
                track_name = Some(String::from_utf8_lossy(name).to_string());
            }
            MetaMessage::Text(txt) => {
                text.push(String::from_utf8_lossy(txt).to_string());
            }
            MetaMessage::Copyright(txt) => {
                copyright.push(String::from_utf8_lossy(txt).to_string());
            }
            MetaMessage::Tempo(tmp) => {
                assert_eq!(tempo, None, "only one tempo is supported");
                tempo = Some(tmp.as_int());
            }
            MetaMessage::TimeSignature(
                numerator,
                denominator,
                clocks_per_tick,
                thirty_seconds_per_beat,
            ) => {
                assert_eq!(time_signature, None, "only one time signature is supported");
                time_signature = Some((
                    *numerator,
                    *denominator,
                    *clocks_per_tick,
                    *thirty_seconds_per_beat,
                ));
            }
            MetaMessage::KeySignature(key, scale) => {
                assert_eq!(key_signature, None, "only one key signature is supported");
                key_signature = Some((*key, *scale));
            }
            MetaMessage::EndOfTrack => {}
            MetaMessage::SequencerSpecific(data) => {
                eprintln!("Unused SequencerSpecific metadata: {:?}", data)
            }
            MetaMessage::SmpteOffset(smpte_time) => {
                eprintln!("Unused SmpteOffset: {:?}", smpte_time)
            }
            MetaMessage::MidiChannel(channel) => {
                eprintln!("Unused MidiChannel: {}", channel)
            }
            MetaMessage::MidiPort(port) => {
                eprintln!("Unused MidiPort: {}", port)
            }
            _ => {
                unimplemented!("unsupported meta event: {:?}", msg)
            }
        }
    }

    // Default BPM is 120 = 500_000 microseconds per beat
    if tempo.is_none() {
        tempo = Some(500_000)
    };

    // ensure!(
    //     track_name.is_some(),
    //     "metadata track must have a track name"
    // );
    ensure!(
        time_signature.is_some(),
        "metadata track must have a time signature"
    );

    Ok((
        next_index,
        MidiMetadata {
            track_name,
            text,
            copyright,
            tempo: tempo.unwrap(),
            time_signature: time_signature.unwrap(),
            key_signature: key_signature.unwrap_or((0, false)), // Default to C major
        },
    ))
}

fn absolutize_track(track: &Track, track_number: u16) -> Vec<AbsoluteMidiEvent> {
    let mut absolute_time = 0;
    let mut events = Vec::with_capacity(track.len());

    for (i, TrackEvent { delta, kind }) in track.iter().enumerate() {
        // Accumulate the absolute time
        let delta_ticks = delta.as_int();
        absolute_time += delta_ticks;

        // Only MIDI events are supported
        let (channel_number, message) = match kind {
            TrackEventKind::Midi { channel, message } => (channel.as_int() + 1, message),
            TrackEventKind::Meta(MetaMessage::EndOfTrack) => {
                if i != track.len() - 1 {
                    eprintln!("Warning: end of track message not at end of track");
                }

                continue;
            }
            _ => {
                eprintln!(
                    "Warning: non-midi message in data track not supported ({:?})",
                    kind
                );
                continue;
            }
        };

        // Convert the MIDI message into our MIDI representation
        let message = match message {
            MidiMessage::NoteOn { key, vel } => LimitedMidiMessage::NoteOn {
                note: key.as_int(),
                velocity: vel.as_int(),
            },
            MidiMessage::NoteOff { key, vel } => LimitedMidiMessage::NoteOff {
                note: key.as_int(),
                velocity: vel.as_int(),
            },
            // MidiMessage::ProgramChange { program } => LimitedMidiMessage::ProgramChange {
            //     program: program.as_int(),
            // },
            // MidiMessage::Controller { controller, value } => LimitedMidiMessage::ControlChange {
            //     control: controller.as_int(),
            //     value: value.as_int(),
            // },
            // MidiMessage::PitchBend { bend } => LimitedMidiMessage::PitchBend {
            //     value: bend.as_int(),
            // },
            _ => {
                eprintln!("Warning: unsupported MIDI message ({:?})", message);
                continue;
            }
        };

        // Push the event back to the list of events
        events.push(AbsoluteMidiEvent {
            time_offset: absolute_time,
            track: track_number,
            channel: channel_number,
            message,
        })
    }

    events
}
