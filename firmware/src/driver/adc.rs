use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
};

use enum_map::{enum_map, EnumMap};
use esp_idf_hal::{
    adc::{Adc, AdcContConfig, AdcContDriver, AdcMeasurement, Attenuated, EmptyAdcChannels},
    gpio::ADCPin,
    peripheral::Peripheral,
    units::Hertz,
};

use crate::adc::*;
use esp_idf_sys::*;

// Minimum frequency to measure 1kHz PWM at 10% duty = 10kHz. Multiply by 4 inputs
const SAMPLING_FREQ_HZ: Hertz = Hertz(10000 * 4);

pub struct AdcDmaDriver {
    adc: Option<AdcContDriver<'static>>,
    channels: EnumMap<AdcChannel, u32>,
    shutdown: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl AdcDmaDriver {
    pub fn new<A: Adc + 'static>(
        adc: impl Peripheral<P = A> + 'static,
        l1_ch: impl ADCPin<Adc = A>,
        l2_ch: impl ADCPin<Adc = A>,
        l3_ch: impl ADCPin<Adc = A>,
        cp_ch: impl ADCPin<Adc = A>,
    ) -> Result<Self, EspError> {
        // Configure ADC unit
        log::info!("Initializing ADC");
        let channels = enum_map! {
            AdcChannel::CurrentL1 => l1_ch.adc_channel(),
            AdcChannel::CurrentL2 => l2_ch.adc_channel(),
            AdcChannel::CurrentL3 => l3_ch.adc_channel(),
            AdcChannel::ControlPilot => cp_ch.adc_channel(),
        };
        let config = AdcContConfig::default()
            .sample_freq(SAMPLING_FREQ_HZ)
            .frame_measurements(100)
            .frames_count(10);
        let channel_config = EmptyAdcChannels::chain(Attenuated::db11(l1_ch))
            .chain(Attenuated::db11(l2_ch))
            .chain(Attenuated::db11(l3_ch))
            .chain(Attenuated::db11(cp_ch));
        let adc = AdcContDriver::new(adc, &config, channel_config)?;

        log::info!("Starting ADC DMA thread");
        // adc.start()?;

        let shutdown = Arc::new(AtomicBool::new(false));

        Ok(Self {
            adc: Some(adc),
            channels,
            join_handle: None,
            shutdown,
        })
    }
}

impl AdcSubscriber for AdcDmaDriver {
    fn subscribe(
        &mut self,
        receiver: impl FnMut(AdcChannel, &mut dyn Iterator<Item = i32>) + Send + 'static,
    ) {
        let mut thread = AdcDmaThread {
            adc: self.adc.take().unwrap(),
            channels: self.channels,
            shutdown: self.shutdown.clone(),
            receiver,
        };
        self.join_handle = Some(thread::spawn(move || thread.run()));
    }
}

impl Drop for AdcDmaDriver {
    fn drop(&mut self) {
        if let Some(handle) = self.join_handle.take() {
            self.shutdown.store(true, Ordering::Relaxed);
            handle.join().unwrap();
        }
    }
}

struct AdcDmaThread<'a, R: FnMut(AdcChannel, &mut dyn Iterator<Item = i32>)> {
    adc: AdcContDriver<'a>,
    channels: EnumMap<AdcChannel, u32>,
    shutdown: Arc<AtomicBool>,
    receiver: R,
}

impl<'a, R: FnMut(AdcChannel, &mut dyn Iterator<Item = i32>)> AdcDmaThread<'a, R> {
    pub fn run(&mut self) {
        let mut chars = esp_adc_cal_characteristics_t::default();
        unsafe {
            esp_adc_cal_characterize(
                1,
                adc_atten_t_ADC_ATTEN_DB_11,
                adc_bits_width_t_ADC_WIDTH_BIT_12,
                0,
                &mut chars as *mut _,
            );
        }

        self.adc.start().unwrap();
        let mut values = [AdcMeasurement::default(); 100];
        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                break;
            };

            if let Ok(num_read) = self.adc.read(&mut values, 10) {
                let iter = &values[0..num_read].iter();
                for (channel, ch) in self.channels {
                    let mut filtered_it = iter.clone().filter_map(|d| {
                        (d.channel() == ch)
                            .then_some((d.data() as u32 * chars.coeff_a / 65536) as i32)
                    });
                    (self.receiver)(channel, &mut filtered_it)
                }
            }
        }
    }
}
