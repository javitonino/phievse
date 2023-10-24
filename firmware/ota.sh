#!/bin/sh

set -e

cargo build
espflash save-image ESP32-C3 target/riscv32imc-esp-espidf/debug/phievse _img
curl -X POST phievse/ota/update --data-binary @_img
rm _img
