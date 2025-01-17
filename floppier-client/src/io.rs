use alloc::vec::Vec;

use rp_pico::hal::usb::UsbBus;
use usbd_serial::SerialPort;

use floppier_proto::{FloppierC2SMessage, FloppierS2CMessage};

static mut READ_BUFFER: Vec<u8> = Vec::new();
static mut READ_BUFFER_LEN: usize = 0;

/// Update the read buffer with any new data from the serial port
///
/// This gets called during USB event interrupts because data packets are sometimes split across
/// multiple USB packets. This function will read the data from the serial port and append it to the
/// internal read buffer until a full message has been received.
pub fn update_read_buffer(serial: &mut SerialPort<UsbBus>) {
    let mut buf = [0u8; 64];
    let count = match serial.read(&mut buf) {
        Err(_) | Ok(0) => return,
        Ok(count) => count,
    };

    #[cfg(feature = "io_debug")]
    {
        defmt::debug!("received {} bytes", count);
        defmt::debug!("buf: {:?}", &buf[..count]);
    }

    let read_buffer = unsafe { &mut READ_BUFFER };
    let read_buffer_len = unsafe { &mut READ_BUFFER_LEN };

    // If a length hasn't been read yet, read the first two bytes as a length, and the rest as data
    if *read_buffer_len == 0 {
        assert!(
            count >= 2,
            "Expected at least 2 bytes when read buffer is empty. Got {}",
            count
        );

        let len_bytes = &buf[..2];

        let len = u16::from_le_bytes([len_bytes[0], len_bytes[1]]) as usize;

        read_buffer.clear();
        read_buffer.reserve(len);

        *read_buffer_len = len;

        read_buffer.extend_from_slice(&buf[2..count]);
    } else {
        read_buffer.extend_from_slice(&buf[..count]);
    }

    assert!(
        read_buffer.len() <= *read_buffer_len,
        "Caught read buffer overflow!"
    );
}

/// Get the received message from the read buffer if one has been fully received
///
/// Must be called after a call to `update_read_buffer` to ensure that the read buffer
/// does not overflow and is up to date
pub fn get_received_message() -> Option<FloppierS2CMessage> {
    let read_buffer = unsafe { &mut READ_BUFFER };
    let read_buffer_len = unsafe { &mut READ_BUFFER_LEN };

    if read_buffer.is_empty() || read_buffer.len() != *read_buffer_len {
        return None;
    }

    // debug!("read buffer: {:?}", read_buffer);
    // debug!("read buffer len: {}", read_buffer.len());

    let message = ciborium::from_reader(&read_buffer[..])
        .expect("Failed to parse a message from the read buffer!");

    #[cfg(feature = "io_debug")]
    {
        defmt::debug!("received message: {:?}", message);
    }

    read_buffer.clear();
    *read_buffer_len = 0;

    Some(message)
}

/// Send a message to the server over USB serial
pub fn send_message(
    serial: &mut SerialPort<UsbBus>,
    message: FloppierC2SMessage,
) -> Result<(), ()> {
    let mut data = Vec::new();
    ciborium::into_writer(&message, &mut data).map_err(|_| ())?;

    let mut buf = Vec::with_capacity(data.len() + 2);

    buf.extend_from_slice(&(data.len() as u16).to_le_bytes());
    buf.extend(data);

    let mut wr_ptr = &buf[..];
    while !wr_ptr.is_empty() {
        let _ = serial.write(wr_ptr).map(|len| {
            wr_ptr = &wr_ptr[len..];
        });
    }
    Ok(())
}
