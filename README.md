# thermo

<img alt="Thermal printer with a piece of paper coming out with the text 'Hello World'. On top of the printer is a 3d printed, crab, the mascott of the programming language Rust. In the background of the image are wires connecting the printer to a Raspberry Pi 2 W" src="thermo.jpg" width="440px"/>

Control a thermal printer with a Raspberry Pi using Rust and WebSockets.

This repo has two main components:

- Relay server to send print jobs to.
- Client on the Pi, reading print jobs from the relay and operating the printer.

## Bill of Materials

1. Soldering iron and solder
2. Raspberry Pi W with power supply and microSD card
3. QR 204 Thermal printer, baud rate 9600, TTL, page width: 58mm
4. Paper rolls for the printer
5. 5-9V, 2A universal power adapter 5.5\*2.1/2.5mm
6. Panel-mount DC-022B female DC power jack (5,5 × 2,1 mm / 5,5 × 2,5 mm) with JST 2-pin male plug (2-pin pitch) on a 15 cm wire lead.

## Assembly

### Wiring

You can not power the printer with the Pi's GPIOs.  
Instead use the universal power adapter (5.), plug that into the female DC power jack (6.) and connect it's VCC and GND to the printers VIN and GND.

For controling the printer, wire Pi and printer like this:

| Printer | Pi            |
| ------- | ------------- |
| GND     | GND           |
| RX      | TXD (gpio 14) |

The printer comes with a 5-pin JST connector (male-male), but you only need GND and RX.

## Software

The project has two parts:

- `server/`: a print relay server with basic auth
- `os/`: the printer client that connects to the relay over websocket and prints incoming jobs

### Requirements

You need [rust](https://www.rust-lang.org/tools/install), [docker](https://docs.docker.com/get-docker/) and, for convenience, [just](https://just.systems/) installed on your dev machine.

## Server

Rename `.env.example` to `.env` in `server/`. Alter the values if you want, but thats optional.

Run the relay server from `server/` with `cargo run` or build a binary with `cargo build --release`.

The relay exposes:

- `POST /print` to submit a print job as JSON: `{ "text": "hello" }`
- `GET /print/ws` as the authenticated websocket endpoint the printer connects to

### OS

Use ssh to connect to your pi, run `sudo raspi-config`, navigate to `Interfacing Options` and **dis**able the `Serial Port` login shell and **en**able the serial hardware.  
Reboot the Pi with `sudo reboot` to apply the changes.

Rename `.env.example` to `.env` in `os/` and alter the env vars:

- `PI_USER`: user of your pi
- `PI_IP`: ip address of your pi
- `SERVER_URL`: url of your relay server, e.g. `http://localhost:3000`
- `BASIC_AUTH_USER` and `BASIC_AUTH_PASSWORD`: credentials for the relay server

Make sure the basic auth credentials used here match the ones set via env vars in `os/`

Then run `just run-remote` from `os/` to crosscompile it inside docker, send it to the Pi via scp and run it there.

Finally, send a print job to the relay server to print "Hello, World!":

```bash
curl -X POST \
    -H "Content-Type: application/json" \
    -u "thermo:thermo" \
    -d '{"text": "Hello, World!"}' \
    http://localhost:3000/print
```

(Make sure to adjust the credentials and url if you changed them in the env vars)
