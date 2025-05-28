use std::{
    cmp::{max, min},
    f32::consts::SQRT_2,
    sync::atomic::AtomicU32,
};

const DEADZONE_MV: i32 = 100;
const WAVELENGTH: i32 = 200;

const CT_RATIO: f32 = 600.0;
const SHUNT_RESISTOR: f32 = 15.0;

#[derive(Debug)]
pub struct CurrentMeter {
    count: i32,
    stats: &'static AtomicU32,
    min: i32,
    max: i32,
    wave_count: i32,
    wave_sum: i32,
    peak_mv_to_rms_ma: f32,
}

impl CurrentMeter {
    pub fn new(stats: &'static AtomicU32, extra_resistor: f32) -> Self {
        Self {
            count: 0,
            stats,
            min: 10000,
            max: 0,
            wave_count: 0,
            wave_sum: 0,
            peak_mv_to_rms_ma: CT_RATIO / (SHUNT_RESISTOR + extra_resistor) / SQRT_2 / 2.0,
        }
    }

    pub fn receive(&mut self, data: &mut dyn Iterator<Item = i32>) {
        for d in data {
            self.count += 1;
            self.min = min(self.min, d);
            self.max = max(self.max, d);

            if self.count > WAVELENGTH {
                self.wave_count += 1;
                self.wave_sum += self.max - self.min;
                self.min = 10000;
                self.max = 0;
                self.count = 0;
            }
        }

        // Average all waves every second
        if self.wave_count >= 50 {
            let peak_to_peak_mv = self.wave_sum / self.wave_count;
            let rms_ma = if peak_to_peak_mv > DEADZONE_MV {
                peak_to_peak_mv as f32 * self.peak_mv_to_rms_ma
            } else {
                0.0
            };
            self.stats
                .store(rms_ma as u32, std::sync::atomic::Ordering::Relaxed);

            // Reset
            self.wave_sum = 0;
            self.wave_count = 0;
        }
    }
}
