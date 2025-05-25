use std::{
    cmp::{max, min},
    sync::atomic::AtomicU32,
};

const VREF_MV: i32 = 1200;
const DEADZONE_MV: i32 = 70;
const WAVELENGTH: i32 = 200;
const WAVELENGTH_ERROR: i32 = 20;

const CT_RATIO: f32 = 600.0;
const SHUNT_RESISTOR: f32 = 15.0;

#[derive(Debug)]
enum State {
    Calibration,
    Idle,
    WaitingPosEdge(bool),
    Active(bool),
}

#[derive(Debug)]
pub struct CurrentMeter {
    vref: i32,
    count: i32,
    square_sum: i32,
    state: State,
    stats: &'static AtomicU32,
    min: i32,
    max: i32,
    rms_mv_to_ma: f32,
}

fn sqrt(n: i32) -> i32 {
    if n < 2 {
        return n;
    }

    // Use Newtom's method on: x^2 - n = 0
    // each iteration the guess is adjusted by f(x) / f'(x)
    // in this case (x²-n)/2x = (x - n/x)/2
    let mut x = 270; // Start point, equivalent to 10A, in the middle of our range

    // 5 iterations seems to be enough for the numbers we are working with
    x -= (x * x - n) / 2 / x;
    x -= (x * x - n) / 2 / x;
    x -= (x * x - n) / 2 / x;
    x -= (x * x - n) / 2 / x;
    x -= (x * x - n) / 2 / x;

    x
}

impl CurrentMeter {
    pub fn new(stats: &'static AtomicU32, extra_resistor: f32) -> Self {
        Self {
            vref: VREF_MV,
            count: 0,
            square_sum: 0,
            state: State::Calibration,
            stats,
            min: 10000,
            max: 0,
            rms_mv_to_ma: CT_RATIO / (SHUNT_RESISTOR + extra_resistor),
        }
    }

    pub fn receive(&mut self, data: &mut dyn Iterator<Item = i32>) {
        match self.state {
            State::Calibration => self.calibrate(data),
            State::Idle => self.idle(data),
            _ => self.process(data),
        }
    }

    /// On startup, adjust Vref to be the average of measurements
    fn calibrate(&mut self, data: &mut dyn Iterator<Item = i32>) {
        for d in data {
            self.square_sum += d;
            self.count += 1;
            self.min = min(self.min, d);
            self.max = max(self.max, d);
        }

        if self.count >= WAVELENGTH * 25 {
            self.vref = self.square_sum / self.count;
            // println!("Calibrated to {} ({}-{})", self.vref, self.min, self.max);
            self.reset();
            self.state = State::Idle;
        }
    }

    /// Waits until the reading goes out the deadzone
    fn idle(&mut self, data: &mut dyn Iterator<Item = i32>) {
        let (min, max) = data.fold((10000, 0), |acc, d| (min(acc.0, d), max(acc.0, d)));
        // println!("Idle {} {}", min, max);

        if min < (self.vref - DEADZONE_MV) {
            // println!("No longer idle");
            self.state = State::WaitingPosEdge(false)
        } else if max > (self.vref + DEADZONE_MV) {
            // println!("No longer idle");
            self.state = State::WaitingPosEdge(true)
        }
    }

    /// Normal processing of data
    fn process(&mut self, data: &mut dyn Iterator<Item = i32>) {
        for d in data {
            let over = d > (self.vref + DEADZONE_MV);
            let under = d < (self.vref - DEADZONE_MV);

            self.count += 1;

            match self.state {
                // Wait for a positive edge, doing nothing
                State::WaitingPosEdge(prev_over) => {
                    if self.count > WAVELENGTH + WAVELENGTH_ERROR {
                        // Wave too long - reset
                        // println!("Wave too long {}", self.count);
                        self.state = State::Calibration;
                        self.reset();
                        self.stats.store(0, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }

                    if prev_over && under {
                        self.state = State::WaitingPosEdge(false)
                    } else if !prev_over && over {
                        self.state = State::Active(true);
                        self.reset();
                    }
                }

                // We are calculating a wave
                State::Active(prev_over) => {
                    self.square_sum += (d - self.vref).pow(2);

                    if self.count > WAVELENGTH + WAVELENGTH_ERROR {
                        // Wave too long - reset
                        // println!("Wave too long {}", self.count);
                        self.state = State::Calibration;
                        self.reset();
                        self.stats.store(0, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }

                    if prev_over && under {
                        self.state = State::Active(false)
                    } else if !prev_over && over {
                        // New positive edge, calculate RMS and rest
                        if self.count < WAVELENGTH - WAVELENGTH_ERROR {
                            // Wave too short - reset
                            // println!("Wave too short {}", self.count);
                            self.state = State::Calibration;
                            self.reset();
                            self.stats.store(0, std::sync::atomic::Ordering::Relaxed);
                            return;
                        }

                        let rms = sqrt(self.square_sum / self.count);
                        let rms_ma = rms as f32 * self.rms_mv_to_ma;
                        self.stats
                            .store(rms_ma as u32, std::sync::atomic::Ordering::Relaxed);

                        self.state = State::Active(true);
                        self.reset();
                    }
                }
                _ => {}
            }
        }
    }

    fn reset(&mut self) {
        self.square_sum = 0;
        self.count = 0;
        self.min = 10000;
        self.max = 0;
    }
}
