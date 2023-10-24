# ϕ-EVSE firmware

This is the firmware for the ϕ-EVSE project. See also the [project README](../README.md).

## Design notes

- This is written in Rust using the crates from the esp-rs project. The libraries have evolved a lot since this code was first written and some things can do in a better way (e.g: ADC without dropping to the C API). It's also my first project in Rust and I have not revisited it, so expect to see a lot of non-idiomatic code.
- Platform-independent code is split into a library so it can be reused and tests can run on the host machine.
- Networking and configuration uses the ESP-specific libraries instead of the Rust ecosystem. The main reason for this is image size, since the Rust alternatives are larger and iamge space is already quite tight.

## Building instructions

Use a recent nightly compiler and run `cargo build`. First flash can be done using the `flash_bootloader.sh` script. Further flashes can be done via OTA with the `ota.sh` script (after shutting down the charger from the Web UI).

## Configuration

The controller will expose a `phievse` Wifi AP. You can connect to the web interface via `http://192.168.71.1` and configure the connectivity to another Wifi network from there.