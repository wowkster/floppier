# floppier
Floppier is an embedded Rust project targetting the [Raspberry Pi Pico](https://www.raspberrypi.com/products/raspberry-pi-pico/) which allows you to play MIDI files on a series of 1 or more Floppy Disk Drives.
This project is inspired by [Moppy](https://github.com/Sammy1Am/Moppy2) which is a similar project written in Java and C++ targetting the [Arduino](https://www.arduino.cc/) platform, and the almighty [Floppotron](https://floppotron.com/).

This project differs from previous implementations by allowing you to expand to a large number of drives and plans to have support for other instruments as well such as Hard Disk Drives and Flatbed Scanners.

> [!WARNING]
> This project is still acirtvely in development, so a lot is subject to change 

## Project Structure

This project is organized as a Cargo workspace with the following directories:
- `floppier-server` - A Rust program that runs on a laptop or desktop to parse a MIDI file and send MIDI events over USB to the Pico(s)
- `floppier-client` - An Embedded Rust program that receives MIDI events from the server over USB and is responsible for controlling the individual Floppy Disk Drives
- `floppier-proto` - A Rust library that contains shared protocol data structures which are sent in USB packets
- `midi` - A directory containing some sample MIDI songs and their configuration files
- 
