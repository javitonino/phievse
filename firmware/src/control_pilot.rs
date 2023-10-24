use std::sync::atomic::{AtomicBool, AtomicU32};

use embedded_hal::PwmPin;

pub enum ControlPilotSignal {
    Standby,
    Charge(u32),
    Error,
}

pub fn set_control_pilot(pin: &mut impl PwmPin<Duty = u32>, signal: ControlPilotSignal) {
    let duty: u32 = match signal {
        ControlPilotSignal::Standby => 16383,
        ControlPilotSignal::Charge(mamps @ 6000..=32000) => current_to_duty(mamps),
        ControlPilotSignal::Error => 0,
        _ => 0, // Invalid
    };
    log::info!("Setting duty to {} / {}", duty, pin.get_max_duty());
    pin.set_duty(duty);
}

fn current_to_duty(ma: u32) -> u32 {
    match ma {
        0..=5999 => 0,
        6000..=10999 => 17 * ma / 1000 + 110,
        11000..=13499 => 5051 + ma * ma / 100 * 368 / 100000 - 837 * ma / 1000,
        13500..=32000 => 235 * ma / 1000 - 2713,
        _ => 0
    }
}

#[derive(Debug, PartialEq)]
pub enum ControlPilotMode {
    NotConnected,
    Connected,
    Ready,
    Error,
}

#[derive(Default)]
pub struct ControlPilotReader {
    cp_mv: AtomicU32,
    pub negative: AtomicBool,
}

impl ControlPilotReader {
    pub fn receive(&self, data: &mut dyn Iterator<Item = u32>) {
        if let Some(max) = data.max() {
            self.cp_mv.store(max, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn state(&self) -> ControlPilotMode {
        // TODO: Pilot check triggers
        // if self.negative.load(std::sync::atomic::Ordering::Relaxed) {
        //     return ControlPilotMode::Error;
        // }
        let x = self.cp_mv.load(std::sync::atomic::Ordering::Relaxed);
        match x {
            0..=50 => ControlPilotMode::NotConnected, // < 10
            51..=650 => ControlPilotMode::Connected, // ~ 450
            651.. => ControlPilotMode::Ready, // ~ 1300 (normal) // ~ 2600 (ventilation)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ops::RangeInclusive;
    use crate::control_pilot::current_to_duty;

    #[test]
    fn empirical_values() {
        const TEST_VALUES: [(u32,RangeInclusive<u32>);11] = [
            (6, 211..=213),
            (7, 215..=230),
            (8, 240..=250),
            (9, 250..=270),
            (10, 270..=290),
            (11, 290..=300),
            (12, 300..=310),
            (13, 360..=400),
            (14, 525..=585),
            (15, 740..=830),
            (16, 1030..=1150),
        ];

        for (amps, range) in TEST_VALUES {
            let cp = current_to_duty(amps * 1000);
            assert!(range.contains(&cp));
        }
    }
}
