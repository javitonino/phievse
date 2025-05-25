use adc::{AdcChannel, AdcSubscriber};
use control_pilot::{set_control_pilot, ControlPilotMode, ControlPilotReader, ControlPilotSignal};
use current_meter::CurrentMeter;
use embedded_hal::{digital::v2::InputPin, PwmPin};
use serde::Serialize;
use std::{
    cmp::min,
    fmt::Display,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::sleep,
    time::Duration,
};

use gpio::{AlarmInput, AlarmReceiver, RelayPin};
use watchdog::Watchdog;

pub mod adc;
mod control_pilot;
mod current_meter;
pub mod gpio;
pub mod led;
pub mod logger;
pub mod watchdog;

#[cfg(target_arch = "riscv32")]
pub mod driver;

pub enum ControlMessage {
    SetMaxPower(u32),
    Shutdown,
}

pub struct PhiEvsePeripherals<A, CP, CPN, R1, R2, L1, L2, L3, L3S, W>
where
    A: AdcSubscriber,
    CP: PwmPin,
    CPN: AlarmInput,
    R1: PwmPin<Duty = u32> + Send,
    R2: PwmPin<Duty = u32> + Send,
    L1: InputPin,
    L2: InputPin,
    L3: InputPin,
    L3S: InputPin,
    W: Watchdog,
{
    // /// Main contactor
    pub relay_main: RelayPin<R1>,
    // /// Relay that switches phases 2 and 3 (to switch between 1 and 3 phase mode)
    pub relay_3_phase: RelayPin<R2>,
    /// Current meters for each phase + Control Pilor voltage
    pub analog: A,
    // pilot_positive: xxx
    // /// Control pilot -12v meter (diode check)
    pub pilot_negative: CPN,
    // /// Control pilot PWM pin
    pub control_pilot: CP,
    // /// Voltage sense for each phase
    pub v_sense: (L1, L2, L3),
    // /// 3-phase voltage sense
    pub v_sense_3_phase: L3S,

    pub watchdog: W,
}

#[derive(PartialEq, Debug, Copy, Clone, Default, Serialize)]
pub enum PhiEvseState {
    #[default]
    NotConnected,
    Connected,
    Ready,
    Charging,
    Error,
    Stopping,
    ShuttingDown,
    Shutdown,
}

impl Display for PhiEvseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{self:?}"))
    }
}

#[derive(Clone, Default, Serialize)]
pub struct PhiEvseStatus {
    pub power: u32,
    pub state: PhiEvseState,
    pub max_power: u32,
}

pub struct PhiEvseController<A, CP, CPN, R1, R2, L1, L2, L3, L3S, W>
where
    A: AdcSubscriber,
    CP: PwmPin,
    CPN: AlarmInput,
    R1: PwmPin<Duty = u32> + Send,
    R2: PwmPin<Duty = u32> + Send,
    L1: InputPin,
    L2: InputPin,
    L3: InputPin,
    L3S: InputPin,
    W: Watchdog,
{
    peripherals: PhiEvsePeripherals<A, CP, CPN, R1, R2, L1, L2, L3, L3S, W>,

    current: [AtomicU32; 3],
    control_pilot: ControlPilotReader,
    state: PhiEvseState,
    max_power: u32,
    max_current: u32,
    three_phase: bool,
    current_adjustment: i32,

    status: Arc<Mutex<PhiEvseStatus>>,

    control_tx: mpsc::Sender<ControlMessage>,
    control_rx: mpsc::Receiver<ControlMessage>,
}

impl<A, CP, CPN, R1, R2, L1, L2, L3, L3S, W>
    PhiEvseController<A, CP, CPN, R1, R2, L1, L2, L3, L3S, W>
where
    A: AdcSubscriber,
    CP: PwmPin<Duty = u32>,
    CPN: AlarmInput,
    R1: PwmPin<Duty = u32> + Send,
    R2: PwmPin<Duty = u32> + Send,
    L1: InputPin,
    L2: InputPin,
    L3: InputPin,
    L3S: InputPin,
    W: Watchdog,
{
    pub fn new(peripherals: PhiEvsePeripherals<A, CP, CPN, R1, R2, L1, L2, L3, L3S, W>) -> Self {
        let (tx, rx) = mpsc::channel();

        Self {
            current: Default::default(),
            peripherals,
            control_pilot: Default::default(),
            state: PhiEvseState::NotConnected,
            max_power: 0,
            max_current: 0,
            three_phase: false,
            status: Default::default(),
            current_adjustment: 0,
            control_tx: tx,
            control_rx: rx,
        }
    }

    pub fn status(&self) -> Arc<Mutex<PhiEvseStatus>> {
        self.status.clone()
    }

    pub fn control_channel(&self) -> mpsc::Sender<ControlMessage> {
        self.control_tx.clone()
    }

    pub fn run(&'static mut self) -> ! {
        let mut set_control_pilot =
            |signal| set_control_pilot(&mut self.peripherals.control_pilot, signal);

        // Initialize current meters / ADC
        let mut current_meters = [
            CurrentMeter::new(&self.current[0], 0.8),
            CurrentMeter::new(&self.current[1], 1.4),
            CurrentMeter::new(&self.current[2], 1.6),
        ];
        let cp = &self.control_pilot;
        self.peripherals.analog.subscribe(move |c, d| match c {
            AdcChannel::CurrentL1 => current_meters[0].receive(d),
            AdcChannel::CurrentL2 => current_meters[1].receive(d),
            AdcChannel::CurrentL3 => current_meters[2].receive(d),
            AdcChannel::ControlPilot => cp.receive(d),
        });

        // Initialize CP negative alarm
        let cp_negative = &self.control_pilot.negative;
        self.peripherals
            .pilot_negative
            .subscribe(AlarmReceiver(Box::new(|| {
                cp_negative.store(true, std::sync::atomic::Ordering::Relaxed)
            })));

        set_control_pilot(ControlPilotSignal::Standby);

        // TODO: Safety checks

        // Wait for everything to settle before reading CP and enabling watchdogs/alarms
        sleep(Duration::from_millis(500));
        self.peripherals.pilot_negative.arm();
        self.peripherals.watchdog.init(Duration::from_secs(2));

        // State machine
        let mut i = 0;
        let mut changing_power = false;
        let mut next_current_adjustment = 0;
        let mut prev_state = PhiEvseState::NotConnected;
        let mut stop_timeout = 0;
        loop {
            i = (i + 1) % 50;
            self.peripherals.watchdog.reset();

            // Receive commands
            if let Ok(msg) = self.control_rx.try_recv() {
                match msg {
                    ControlMessage::SetMaxPower(watts) => {
                        self.max_power = match watts {
                            0..=1499 => 0,
                            1500..=11000 => watts,
                            _ => 11000,
                        };
                        self.status.lock().unwrap().max_power = self.max_power;
                        (self.max_current, self.three_phase) = calculate_power(self.max_power);
                        changing_power = true;
                        log::info!(
                            "Setting max power to {}W ({} mA, 3p={})",
                            watts,
                            self.max_current,
                            self.three_phase
                        );
                    }
                    ControlMessage::Shutdown => {
                        if self.state == PhiEvseState::Charging {
                            self.state = PhiEvseState::ShuttingDown;
                            stop_timeout = 50;
                        } else {
                            self.state = PhiEvseState::Shutdown;
                        }
                    }
                }
            }

            // Check safety indicators first
            let cp_state = self.control_pilot.state();
            if cp_state == ControlPilotMode::Error && self.state != PhiEvseState::Error {
                log::error!("Pilot check failed, IGNORE");
            }
            if !self.peripherals.pilot_negative.is_high() {
                log::error!("Pilot negative is low right now, STOP");
                self.state = PhiEvseState::Error;
            }

            match self.state {
                PhiEvseState::NotConnected | PhiEvseState::Connected => {
                    // Wait until EV is connected and ready to charge
                    self.state = match cp_state {
                        ControlPilotMode::NotConnected => PhiEvseState::NotConnected,
                        ControlPilotMode::Connected => PhiEvseState::Connected,
                        ControlPilotMode::Ready => PhiEvseState::Ready,
                        ControlPilotMode::Error => PhiEvseState::Error,
                    };

                    if changing_power && self.state == PhiEvseState::Connected {
                        if self.max_current > 6000 {
                            set_control_pilot(ControlPilotSignal::Charge(
                                (self.max_current as i32 + self.current_adjustment) as u32,
                            ));
                        } else {
                            set_control_pilot(ControlPilotSignal::Standby);
                        }
                    }
                }
                PhiEvseState::Ready => {
                    // Start charging
                    if self.max_current > 6000 {
                        sleep(Duration::from_millis(500)); // Wait a bit or the car gets angry at us for switching the relay too soon
                        self.peripherals
                            .relay_3_phase
                            .set_level_and_wait(self.three_phase);
                        self.peripherals.relay_main.set_level(true);
                        self.state = PhiEvseState::Charging;
                    }
                }
                PhiEvseState::Charging => {
                    if changing_power {
                        set_control_pilot(ControlPilotSignal::Charge(
                            (self.max_current as i32 + self.current_adjustment) as u32,
                        ));
                        next_current_adjustment = 50;

                        if self.three_phase != self.peripherals.relay_3_phase.level() {
                            self.peripherals.relay_3_phase.set_level(self.three_phase);
                            next_current_adjustment = 100; // Give additional time to settle
                        }
                    }

                    let total_mamps: u32 =
                        self.current.iter().map(|c| c.load(Ordering::Relaxed)).sum();
                    self.status.lock().unwrap().power = total_mamps * 230 / 1000;
                    if i == 0 {
                        log::info!(
                            "Charging at {:?} mamps / ADJ = {}",
                            self.current
                                .iter()
                                .map(|c| c.load(Ordering::Relaxed))
                                .collect::<Vec<u32>>(),
                            self.current_adjustment
                        )
                    };

                    if next_current_adjustment == 0 {
                        // Check if EV wants to stop charging
                        if cp_state != ControlPilotMode::Ready {
                            self.state = PhiEvseState::Stopping;
                            stop_timeout = 50;
                        } else {
                            let phases = if self.three_phase { 3 } else { 1 };
                            let mamps_per_phase = total_mamps / phases;
                            let current_diff: i32 =
                                self.max_current as i32 - mamps_per_phase as i32;
                            if mamps_per_phase < 1000 {
                                // Not yet charging, wait before adjusting
                                // TODO: Abort if waiting for too long for charge to start?
                                next_current_adjustment += 1;
                            } else if mamps_per_phase > self.max_current + 4000 {
                                // Car drawing way too much current, emergency shutdown
                                log::warn!(
                                    "Car pulling {} mamps while maximum allowed is {}. Stop!",
                                    mamps_per_phase,
                                    self.max_current
                                );
                                self.state = PhiEvseState::Error;
                            } else if mamps_per_phase < 6500 {
                                // Current close to minimum, increase to avoid cut-off
                                self.current_adjustment += 500;
                                if self.current_adjustment.abs() > 1000 {
                                    self.current_adjustment =
                                        self.current_adjustment.signum() * 1000
                                }
                                log::info!(
                                    "Adjusting over 6500. ADJ = {}",
                                    self.current_adjustment
                                );
                                set_control_pilot(ControlPilotSignal::Charge(
                                    (self.max_current as i32 + self.current_adjustment) as u32,
                                ));
                                next_current_adjustment = 30;
                            } else if current_diff.abs() > 500 {
                                self.current_adjustment += current_diff.signum() * 300;
                                if self.current_adjustment.abs() > 1000 {
                                    self.current_adjustment =
                                        self.current_adjustment.signum() * 1000
                                }
                                log::info!(
                                    "Adjusting 500 mamps diff. ADJ = {} ",
                                    self.current_adjustment
                                );
                                set_control_pilot(ControlPilotSignal::Charge(
                                    (self.max_current as i32 + self.current_adjustment) as u32,
                                ));
                                next_current_adjustment = 30;
                            }
                        }
                    } else {
                        next_current_adjustment -= 1;
                    }
                }
                PhiEvseState::Stopping | PhiEvseState::ShuttingDown => {
                    // Wait until car stops charging or timeout expires and then disconnect relays
                    let total_mamps: u32 =
                        self.current.iter().map(|c| c.load(Ordering::Relaxed)).sum();
                    if total_mamps == 0 || stop_timeout <= 0 {
                        self.peripherals.relay_main.set_level_and_wait(false);
                        self.peripherals.relay_3_phase.set_level(false);

                        if self.state == PhiEvseState::Stopping {
                            self.state = match cp_state {
                                ControlPilotMode::NotConnected => PhiEvseState::NotConnected,
                                ControlPilotMode::Connected => PhiEvseState::Connected,
                                ControlPilotMode::Ready => unreachable!(),
                                ControlPilotMode::Error => PhiEvseState::Error,
                            };
                        } else {
                            self.state = PhiEvseState::Shutdown;
                        }
                    } else {
                        stop_timeout -= 1;
                    }
                }
                PhiEvseState::Error => {
                    // Wait until EV is disconnected to clean the error
                    if cp_state == ControlPilotMode::NotConnected {
                        self.state = PhiEvseState::NotConnected;
                    }
                }
                PhiEvseState::Shutdown => {}
            }
            changing_power = false;

            // Actions when entering a new state
            if prev_state != self.state {
                log::info!("State transition {:?} => {:?}", prev_state, self.state);

                match self.state {
                    PhiEvseState::NotConnected => {
                        set_control_pilot(ControlPilotSignal::Standby);
                    }
                    PhiEvseState::Connected => {
                        self.current_adjustment = 1000;
                        if self.max_current > 6000 {
                            set_control_pilot(ControlPilotSignal::Charge(
                                (self.max_current as i32 + self.current_adjustment) as u32,
                            ));
                        }
                    }
                    PhiEvseState::Ready => {
                        // set_control_pilot(ControlPilotSignal::Charge(self.max_current));
                    }
                    PhiEvseState::Charging => {}
                    PhiEvseState::Error => {
                        set_control_pilot(ControlPilotSignal::Error);
                        self.peripherals.relay_main.set_level_and_wait(false);
                        self.peripherals.relay_3_phase.set_level(false);
                    }
                    PhiEvseState::Stopping | PhiEvseState::ShuttingDown => {
                        set_control_pilot(ControlPilotSignal::Standby);
                    }
                    PhiEvseState::Shutdown => {
                        self.peripherals.watchdog.stop();
                    }
                }

                self.status.lock().unwrap().state = self.state;
                prev_state = self.state;
            }

            sleep(Duration::from_millis(100));
        }
    }
}

fn calculate_power(watts: u32) -> (u32, bool) {
    let total_mamps = watts * 1000 / 230;
    match total_mamps {
        0..=6499 => (0, false),
        6500..=19999 => (min(total_mamps, 16000), false),
        20000..=47999 => (total_mamps / 3, true),
        _ => unreachable!(),
    }
}
