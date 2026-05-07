# thermo

<img alt="A rectangular 3D printed beige case for a OLED display showing the current time." src="img/demo.jpg" width="440px"/>

## Bill of Materials

- TODO

## Usage

### Wiring

### Software

The project now has two Rust binaries:

- `server/`: an Axum-based authenticated print relay server
- `os/`: the printer client that connects to the relay over websocket and prints incoming jobs

You need [rust](https://www.rust-lang.org/tools/install), [docker](https://docs.docker.com/get-docker/) and, for convenience, [just](https://just.systems/) installed on your dev machine.

Use ssh to connect to your pi, run `sudo raspi-config`, navigate to `Interfacing Options` and **dis**able the `Serial Port` login shell and **en**able the serial hardware:

Reboot the Pi with `sudo reboot` to apply the changes.

Rename `.env.example` to `.env` in `os/` and fill in the required environment variables, including `SERVER_URL`, `BASIC_AUTH_USER`, and `BASIC_AUTH_PASSWORD`.

Run the relay server from `server/`, and then deploy the printer client from `os/` with `just run-remote`.

The relay exposes:

- `POST /print` to submit a print job as JSON: `{ "text": "hello" }`
- `GET /print/ws` as the authenticated websocket endpoint the printer connects to

The server uses `BIND_ADDR`, `BASIC_AUTH_USER`, and `BASIC_AUTH_PASSWORD` from the environment.
