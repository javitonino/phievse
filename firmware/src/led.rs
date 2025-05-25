//! RGB LED controller that allows setting blinking color patterns

use std::error::Error;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(test)]
use mockall::automock;

/// A driver for an RGB LED
#[cfg_attr(test, automock)]
pub trait LedDriver: Send + 'static {
    /// Sets the color of the LED, blocking until it's done.
    fn set_rgb(&self, r: u8, g: u8, b: u8) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug, Clone, Copy)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    const OFF: Self = Self { r: 0, g: 0, b: 0 };
}

/// One step of a LED blinking pattern. Displays a color for a duration.
pub struct LedPatternItem {
    color: Color,
    // Using a std::time::Duration here crashes when sending across threads
    // Maybe it is any type bigger than 4 bytes?. u32 worked but u64 failed
    duration: Box<Duration>,
}

impl LedPatternItem {
    /// Displays the specified color forever
    fn fixed(color: Color) -> Self {
        Self {
            color,
            duration: Box::new(Duration::ZERO),
        }
    }
}

/// A blinking pattern, it consists of a list of colors an durations.
type LedPattern = Vec<LedPatternItem>;

/// LED controller that displays a `LedPatternItem` in a `LedDriver`.
///
/// It spawns a background thread to use as a timer.
pub struct LedController {
    pattern: Arc<Mutex<LedPattern>>,
    tx: Sender<LedControllerCommand>,
    join_handle: Option<JoinHandle<()>>,
}

enum LedControllerCommand {
    Timeout,
    Change,
    Exit,
}

impl Drop for LedController {
    /// Stops the controller thread, will ignore panics but can block if the thread is unresponsive.
    fn drop(&mut self) {
        if let Some(handle) = self.join_handle.take() {
            if let Err(e) = self.tx.send(LedControllerCommand::Exit) {
                log::error!("Could not send exit command to LED thread: {}", e);

                // This normally happens because the thread panic'ed. In tests, we want to capture that
                #[cfg(test)]
                panic!("{}", e);
            } else {
                handle.join().unwrap()
            }
        }
    }
}

impl LedController {
    pub fn new<T: LedDriver>(driver: T) -> Self {
        let (tx, rx): (Sender<LedControllerCommand>, Receiver<LedControllerCommand>) = mpsc::channel();
        let pattern = Arc::new(Mutex::new(vec![LedPatternItem::fixed(Color::OFF)]));

        let thread = LedControllerThread {
            driver,
            pattern: Arc::clone(&pattern),
            rx,
        };
        let handle = thread::spawn(move || thread.run());

        Self {
            pattern,
            tx,
            join_handle: Some(handle),
        }
    }

    /// Sets a fixed color
    pub fn set_color(&self, color: Color) {
        self.set_pattern(vec![LedPatternItem::fixed(color)]);
    }

    /// Sets a color that will blink on and off
    pub fn set_blink(&self, color: Color) {
        self.set_pattern(vec![
            LedPatternItem {
                color,
                duration: Box::new(Duration::from_millis(200)),
            },
            LedPatternItem {
                color: Color::OFF,
                duration: Box::new(Duration::from_millis(200)),
            },
        ]);
    }

    /// Sets an arbitraty pattern
    pub fn set_pattern(&self, new: LedPattern) {
        let pat = &mut *self.pattern.lock().unwrap();
        pat.clear();
        pat.extend(new);
        self.tx
            .send(LedControllerCommand::Change)
            .unwrap_or_else(|e| log::error!("Error notifying LED thread: {}", e));
    }
}

struct LedControllerThread<T: LedDriver> {
    driver: T,
    pattern: Arc<Mutex<Vec<LedPatternItem>>>,
    rx: Receiver<LedControllerCommand>,
}

impl<T: LedDriver> LedControllerThread<T> {
    fn run(&self) {
        let mut i = 0usize;
        loop {
            let wait = {
                let pattern = self.pattern.lock().unwrap_or_else(|e| {
                    println!("Error mutex {e}");
                    panic!("AHHHH");
                });
                i %= pattern.len();
                self.set_color(pattern[i].color);
                *pattern[i].duration
            };

            let command = if wait.is_zero() {
                self.rx.recv().unwrap_or(LedControllerCommand::Timeout)
            } else {
                self.rx.recv_timeout(wait).unwrap_or(LedControllerCommand::Timeout)
            };

            match command {
                LedControllerCommand::Exit => break,
                LedControllerCommand::Change => i = 0,
                LedControllerCommand::Timeout => i += 1,
            }
        }
    }

    fn set_color(&self, c: Color) {
        self.driver
            .set_rgb(c.r, c.g, c.b)
            .unwrap_or_else(|e| println!("Could not set color: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::{Color, LedController, MockLedDriver};
    use mockall::{predicate::*, Sequence};
    use std::{thread, time::Duration};

    #[test]
    fn defaults_to_off() {
        let mut driver = MockLedDriver::new();

        driver
            .expect_set_rgb()
            .with(eq(0), eq(0), eq(0))
            .once()
            .returning(|_, _, _| Ok(()));

        let _led = LedController::new(driver);
        thread::sleep(Duration::from_millis(10));
    }

    #[test]
    fn sets_a_fixed_color() {
        let mut seq = Sequence::new();

        let mut driver = MockLedDriver::new();

        driver
            .expect_set_rgb()
            .with(eq(0), eq(0), eq(0))
            .once()
            .returning(|_, _, _| Ok(()))
            .in_sequence(&mut seq);

        driver
            .expect_set_rgb()
            .with(eq(1), eq(2), eq(3))
            .once()
            .returning(|_, _, _| Ok(()))
            .in_sequence(&mut seq);

        let led = LedController::new(driver);
        thread::sleep(Duration::from_millis(10));

        led.set_color(Color { r: 1, g: 2, b: 3 });
        thread::sleep(Duration::from_millis(10));
    }

    #[test]
    fn sets_a_blinking_color() {
        let mut seq = Sequence::new();

        let mut driver = MockLedDriver::new();

        let mut expect = |n: u8| {
            driver
                .expect_set_rgb()
                .with(eq(n), eq(n), eq(n))
                .once()
                .returning(|_, _, _| Ok(()))
                .in_sequence(&mut seq);
        };

        // Initially OFF
        expect(0);

        // Blinks between 10, 10, 10 and 0, 0, 0 twice
        expect(10);
        expect(0);
        expect(10);
        expect(0);

        let led = LedController::new(driver);
        thread::sleep(Duration::from_millis(10));

        led.set_blink(Color { r: 10, g: 10, b: 10 });
        thread::sleep(Duration::from_millis(700));
    }
}
