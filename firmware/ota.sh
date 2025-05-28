#!/bin/sh

set -e

cargo build
espflash save-image --chip esp32c3 target/riscv32imc-esp-espidf/debug/phievse _img
curl -fX POST phievse/shutdown
curl -fX POST phievse/ota/update --data-binary @_img
rm _img
