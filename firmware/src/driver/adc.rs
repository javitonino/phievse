use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},};

use enum_map::{enum_map, EnumMap};

use crate::adc::*;
use esp_idf_sys::*;

// Minimum frequency to measure 1kHz PWM at 10% duty = 10kHz. Multiply by 4 inputs
const SAMPLING_FREQ_HZ: u32 = 10000 * 4;

// Max items to convert in one go
const DMA_BUFFER_SIZE_ITEMS: usize = 400;
const DMA_BUFFER_SIZE_BYTES: usize = DMA_BUFFER_SIZE_ITEMS * 4;

pub struct AdcDmaDriver {
    channels: EnumMap<AdcChannel, u8>,
    shutdown: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl AdcDmaDriver {
    pub fn new(l1_ch: u8, l2_ch: u8, l3_ch: u8, cp_ch: u8) -> Result<Self, EspError> {
        let channels = [l1_ch, l2_ch, l3_ch, cp_ch];
        // Configure ADC unit
        log::info!("Initializing ADC");
        let adc_config = adc_digi_init_config_s {
            max_store_buf_size: DMA_BUFFER_SIZE_BYTES as u32,
            conv_num_each_intr: DMA_BUFFER_SIZE_ITEMS as u32,
            adc1_chan_mask: channels.iter().fold(0, |acc, c| acc | (1 << c)),
            adc2_chan_mask: 0,
        };
        esp!(unsafe { adc_digi_initialize(&adc_config) })?;

        // Configure DMA patterns
        log::info!("Initializing ADC DMA driver");
        let mut patterns = channels
            .iter()
            .map(|c| adc_digi_pattern_config_t {
                atten: adc_atten_t_ADC_ATTEN_DB_11 as u8,
                channel: *c,
                unit: 0,
                bit_width: 12
            })
            .collect::<Vec<adc_digi_pattern_config_t>>();

        let dma_config = adc_digi_configuration_t {
            sample_freq_hz: SAMPLING_FREQ_HZ,
            conv_limit_en: false,
            conv_limit_num: 0,
            pattern_num: channels.len() as u32,
            adc_pattern: patterns.as_mut_ptr(),
            conv_mode: adc_digi_convert_mode_t_ADC_CONV_SINGLE_UNIT_1,
            format: adc_digi_output_format_t_ADC_DIGI_OUTPUT_FORMAT_TYPE2
        };
        esp!(unsafe { adc_digi_controller_configure(&dma_config) })?;

        log::info!("Starting ADC DMA thread");
        esp!(unsafe { adc_digi_start() })?;

        let shutdown = Arc::new(AtomicBool::new(false));

        let channels = enum_map! {
            AdcChannel::CurrentL1 => l1_ch,
            AdcChannel::CurrentL2 => l2_ch,
            AdcChannel::CurrentL3 => l3_ch,
            AdcChannel::ControlPilot => cp_ch,
        };

        Ok(Self {
            channels,
            join_handle: None,
            shutdown,
        })
    }
}

impl AdcSubscriber for AdcDmaDriver {
    fn subscribe(&mut self, receiver: impl FnMut(AdcChannel, &mut dyn Iterator<Item = u32>) + Send + 'static) {
        let mut thread = AdcDmaThread {
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

struct AdcDmaThread<R: FnMut(AdcChannel, &mut dyn Iterator<Item = u32>)> {
    channels: EnumMap<AdcChannel, u8>,
    shutdown: Arc<AtomicBool>,
    receiver: R,
}

impl<R: FnMut(AdcChannel, &mut dyn Iterator<Item = u32>)> AdcDmaThread<R> {
    pub fn run(&mut self) {
        let mut buf: Box<[adc_digi_output_data_t; DMA_BUFFER_SIZE_ITEMS]> =
            Box::new([Default::default(); DMA_BUFFER_SIZE_ITEMS]);
        let mut len: u32 = 0;

        // Get ADC characteristics to convert readings to mV
        let mut chars = esp_adc_cal_characteristics_t { ..Default::default() };
        unsafe {
            esp_adc_cal_characterize(
                1,
                adc_atten_t_ADC_ATTEN_DB_11,
                adc_bits_width_t_ADC_WIDTH_BIT_12,
                0,
                &mut chars as *mut _,
            );
        }

        // Read from ADC, split by channel and send to subscriber
        loop {
            if self.shutdown.load(Ordering::Relaxed) {
                break;
            };

            unsafe { adc_digi_read_bytes(buf.as_mut_ptr() as *mut u8, DMA_BUFFER_SIZE_BYTES as u32, &mut len, 50) };

            let iter = buf[0..(len / 4) as usize].iter();
            for (channel, ch) in self.channels {
                (self.receiver)(
                    channel,
                    &mut iter.clone().filter_map(|d| {
                        let d = unsafe { d.__bindgen_anon_1.type2 };
                        if d.channel() == ch as u32 {
                            Some(d.data() * chars.coeff_a / 65536)
                        } else {
                            None
                        }
                    }),
                );
            }
        }
    }
}
