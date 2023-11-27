use std::{path::PathBuf, thread, time::Duration};

use anyhow::{bail, Result};
use clap::Parser;
use floppier_proto::{FloppierC2SMessage, FloppierS2CMessage, MidiEvent, SetConfig};

use crate::{
    io::Client,
    midi::{parse_midi_file, ticks_to_microseconds},
};

mod config;
mod io;
mod midi;

/// Server program to drive Floppier hardware client
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct FloppierArgs {
    /// Path to the MIDI configuration file
    #[arg(short, long)]
    pub path: PathBuf,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Serial port configuration
    #[arg(short, long, default_value = "/dev/ttyUSB0")]
    pub serial_port: String,

    /// Serial port baud rate
    #[arg(short, long, default_value_t = 115_200)]
    pub baud_rate: u32,
}

fn main() -> Result<()> {
    /* Parse the CLI arguments and the passed in cong configuration */

    let args = FloppierArgs::parse();
    let config = config::parse_song_config(&args)?;

    /* Parse the midi file into a more easily consumable representation */

    let midi_file = parse_midi_file(&config.midi.path)?;

    println!();
    println!("Parsed MIDI file");
    println!("================");
    println!("{}", &midi_file.metadata);
    println!();

    /* Pause the program and wait for the user to initiate the serial communication */

    pause!("Press any key to start the serial connection...");

    /* List Available Serial Ports */

    println!();
    for port in serialport::available_ports()? {
        println!("{:?}", port);
    }
    println!();

    /* Open a serial connection with the supplied settings */

    let port = args.serial_port;
    let baud_rate = args.baud_rate;

    println!();
    println!("Serial Connection");
    println!("================");
    println!("Port: {}", port);
    println!("Baud Rate: {}", baud_rate);
    println!();

    let serial_port = serialport::new(port, baud_rate).open()?;
    let mut client = Client::new(serial_port);

    /* Check client connection */

    println!("Connecting to client...");

    client.send(FloppierS2CMessage::Hello)?;

    let FloppierC2SMessage::HelloAck = client.receive()? else {
        bail!("expected hello ack message from client");
    };

    println!("Client connection established!");

    /* Send client configuration (pre-start) */

    println!("Configuring client...");

    client.send(FloppierS2CMessage::SetConfig(SetConfig {
        parallel_mode: config.midi.parallel_mode,
        tracks: config
            .tracks
            .iter()
            .map(|track| {
                (
                    track.track,
                    track
                        .channels
                        .iter()
                        .map(|channel| (channel.channel, channel.ports.clone()))
                        .collect(),
                )
            })
            .collect(),
    }))?;

    let FloppierC2SMessage::SetConfigAck = client.receive()? else {
        bail!("expected set config ack message from client");
    };

    println!("Client configured!");

    /* Wait for client to finish resetting */

    println!("Waiting for client to finish resetting...");

    let FloppierC2SMessage::Ready = client.receive()? else {
        bail!("expected ready message from client");
    };

    println!("Client ready!");

    pause!("Press any key to play the track...");

    /* Send the MIDI events to the client */

    // TODO: Group the events by their time offsets
    //       https://docs.rs/itertools/latest/itertools/trait.Itertools.html#method.group_by
    
    let mut last_tick = 0;

    for event in midi_file.events {
        let delta = event.time_offset - last_tick;
        last_tick = event.time_offset;

        if delta > 0 {
            thread::sleep(Duration::from_micros(ticks_to_microseconds(
                delta,
                midi_file.ticks_per_beat,
                midi_file.beats_per_minute,
            )));
        }

        client.send(FloppierS2CMessage::MidiEvent(MidiEvent {
            track: event.track,
            channel: event.channel,
            message: event.message,
        }))?;

        let FloppierC2SMessage::MidiEventAck = client.receive()? else {
            bail!("expected midi event ack from client");
        };
    }

    Ok(())
}
