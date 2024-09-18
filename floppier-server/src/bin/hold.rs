use std::{collections::BTreeMap, thread, time::Duration};

use anyhow::{bail, Result};
use clap::Parser;
use floppier_proto::{
    FloppierC2SMessage, FloppierS2CMessage, LimitedMidiMessage, MidiEvent, ParallelMode, SetConfig,
};

use floppier_server::{io::Client, pause};

/// Server program to drive Floppier hardware client
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct FloppierArgs {
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
        parallel_mode: ParallelMode::Collapse,
        movement: true,
        drive_count: 3,
        tracks: BTreeMap::from([
            (1, BTreeMap::from([(1, vec![0, 1, 2])])),
        ]),
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

    client.send(FloppierS2CMessage::MidiEvent(MidiEvent {
        track: 1,
        channel: 1,
        message: LimitedMidiMessage::NoteOn {
            note: 72,
            velocity: 100,
        },
    }))?;

    let FloppierC2SMessage::MidiEventAck = client.receive()? else {
        bail!("expected midi event ack from client");
    };

    thread::sleep(Duration::from_millis(1_000 * 60 * 5));

    client.send(FloppierS2CMessage::End)?;

    let FloppierC2SMessage::EndAck = client.receive()? else {
        bail!("expected end ack message from client");
    };

    Ok(())
}
