use std::{sync::atomic::AtomicBool, thread::{self, sleep}, time::Duration};
use embedded_hal::PwmPin;

pub struct AlarmReceiver(pub Box<dyn Fn()>);

impl AlarmReceiver {
    pub fn alarm(&self) {
        self.0()
    }
}

pub trait AlarmInput {
    fn subscribe(&mut self, alarm: AlarmReceiver);
    fn arm(&self);
    fn is_high(&self) -> bool;
}

/// Time to go to holding voltage. Coil takes 45ms to switch, let's take a conservative double
const RELAY_SWITCH_DURATION: std::time::Duration = Duration::from_millis(90);

/// Holding voltage (in percentage of nominal voltage)
const RELAY_HOLDING_DUTY: u32 = 85;

pub struct RelayPin<P: PwmPin<Duty = u32>> {
    pin: P,
    state: AtomicBool
}

impl<P: PwmPin<Duty = u32> + Send + 'static> RelayPin<P> {
    pub fn new(mut pin: P) -> Self {
        pin.set_duty(0);
        Self {
            pin,
            state: AtomicBool::new(false)
        }
    }

    pub fn set_level(&mut self, level: bool) {
        let old_level = self.state.swap(level, std::sync::atomic::Ordering::Relaxed);
        if !level {
            // Off
            self.pin.set_duty(0);
        } else if !old_level {
            // Just turned on
            self.pin.set_duty(self.pin.get_max_duty());

            // SAFETY: The pin will live until the application crashes
            // This also creates multiple references to it (inside the threads and outside)
            // but access is controlled with the state boolean.

            // This is probably best solved with ledc fades, but they are not
            // available in esp-idf-sys yet.
            let p: &'static mut P = unsafe { std::mem::transmute(&mut self.pin) };
            let s: &'static AtomicBool = unsafe { std::mem::transmute(&self.state) };
            thread::spawn(move || {
                sleep(RELAY_SWITCH_DURATION);
                if s.load(std::sync::atomic::Ordering::Relaxed) {
                    // If we haven't turned off the relay, set operation duty
                    p.set_duty(p.get_max_duty() / 100 * RELAY_HOLDING_DUTY)
                }
            });
        }
    }

    pub fn set_level_and_wait(&mut self, level: bool) {
        self.set_level(level);
        sleep(RELAY_SWITCH_DURATION * 2);
    }

    pub fn level(&self) -> bool {
        self.pin.get_duty() > 0
    }
}
