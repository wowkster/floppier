use anyhow::{bail, Result};
use floppier_proto::{FloppierC2SMessage, FloppierS2CMessage};
use serialport::SerialPort;

#[macro_export]
macro_rules! pause {
    () => {
        $crate::io::pause_impl(None);
    };
    ($($arg:tt)*) => {
        $crate::io::pause_impl(Some(&format!($($arg)*)));
    };
}

pub fn pause_impl(message: Option<&str>) {
    use std::io::{stdin, stdout, Write};
    use termion::input::TermRead;
    use termion::raw::IntoRawMode;

    println!("{}", message.unwrap_or("Press any key to continue..."));

    let mut stdout = stdout().into_raw_mode().unwrap();
    stdout.flush().unwrap();
    stdin().events().next();
}

pub struct Client {
    port: Box<dyn SerialPort>,
}

impl Client {
    pub fn new(port: Box<dyn SerialPort>) -> Self {
        Self { port }
    }

    pub fn send(&mut self, message: FloppierS2CMessage) -> Result<()> {
        let mut data = Vec::new();

        ciborium::into_writer(&message, &mut data)?;

        let len = data.len() as u16;

        dbg!(&message);
        // dbg!(&len);
        // dbg!(&len.to_le_bytes());
        // dbg!(&data);

        self.port.write_all(&len.to_le_bytes())?;
        self.port.write_all(&data)?;
        self.port.flush()?;

        Ok(())
    }

    pub fn receive(&mut self) -> Result<FloppierC2SMessage> {
        const TIMEOUT_MS: u128 = 10_000;

        let start_time = std::time::Instant::now();

        loop {
            if self.port.bytes_to_read()? > 0 {
                break;
            }

            if start_time.elapsed().as_millis() > TIMEOUT_MS {
                bail!("timed out waiting for client response");
            }
        }

        let len_buf = self.read_bytes(2)?;
        let len = u16::from_le_bytes(len_buf.try_into().unwrap());

        let message_buf = self.read_bytes(len as usize)?;
        let message = ciborium::from_reader(&message_buf[..])?;

        Ok(message)
    }

    fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let bytes_read = self.port.read(&mut buf)?;

        if bytes_read != len {
            bail!("expected {} bytes, got {}", len, bytes_read);
        }

        Ok(buf)
    }
}
