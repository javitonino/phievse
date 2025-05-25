#![allow(unexpected_cfgs)]

use embedded_svc::ipv4::Configuration;
use embedded_svc::wifi::AccessPointConfiguration;
use esp_idf_hal::gpio::{PinDriver, Pull};
use esp_idf_hal::ledc::config::TimerConfig;
use esp_idf_hal::ledc::{LedcDriver, LedcTimerDriver, Resolution};
use esp_idf_hal::prelude::*;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::netif::{EspNetif, NetifConfiguration};
use esp_idf_svc::sntp::EspSntp;
use esp_idf_svc::wifi::WifiDriver;
use esp_idf_sys::*;
use phievse::driver::gpio::InterruptPin;
use phievse::gpio::RelayPin;
use phievse::*;
use std::error::Error;

// Using as library
use phievse::driver::{adc::*, watchdog::*};
use phievse::logger::RingBufferLogger;

// Using directly
mod config;
mod httpd;
mod mqtt;

use embedded_svc::{ipv4::DHCPClientSettings, wifi::ClientConfiguration};
use esp_idf_svc::{netif::NetifStack, wifi::EspWifi};

use crate::config::*;

esp_idf_sys::esp_app_desc! {}

fn main() -> Result<(), Box<dyn Error>> {
    // Disables USB JTAG on GPIO 18/19. Allows to disable pullups in those pins.
    unsafe {
        let usb_jtag_reg: *mut u32 = 1610887192 as *mut u32;
        let mask_bit_9 = u32::MAX - (1 << 9);
        *usb_jtag_reg &= mask_bit_9;
    }

    esp_idf_svc::sys::link_patches();

    // Set maximum priority after timers and driver code
    // Other spawned threads (e.g: LEDController) default to 5 unless changed
    unsafe { vTaskPrioritySet(std::ptr::null_mut(), 19) };

    // Initialize esp-idf-svc logger
    // EspLogger::initialize_default();

    // Initialize our in-memory logger
    let ring_logger: Box<RingBufferLogger> = Default::default();
    let ring_buffer = ring_logger.buffer.clone();
    log::set_boxed_logger(ring_logger)?;
    log::set_max_level(log::LevelFilter::Info);
    println!("Started logger");

    unsafe {
        let cfg = esp_pthread_cfg_t {
            stack_size: 4096,
            prio: 5,
            inherit_cfg: false,
            thread_name: std::ptr::null(),
            pin_to_core: 0,
        };
        esp_pthread_set_cfg(&cfg);
    }

    // Initialize all peripherals and the controller
    let peripherals = esp_idf_hal::peripherals::Peripherals::take().unwrap();
    let pins = peripherals.pins;

    let timer = LedcTimerDriver::new(
        peripherals.ledc.timer0,
        &TimerConfig::default()
            .frequency(1.kHz().into())
            .resolution(Resolution::Bits14),
    )?;

    std::thread::sleep(std::time::Duration::from_secs(1));
    let mut g10 = PinDriver::input(pins.gpio10)?;
    g10.set_pull(Pull::Floating)?;
    let mut g19 = PinDriver::input(pins.gpio19)?;
    g19.set_pull(Pull::Floating)?;
    let mut g7 = PinDriver::input(pins.gpio7)?;
    g7.set_pull(Pull::Floating)?;
    let mut g5 = PinDriver::input(pins.gpio5)?;
    g5.set_pull(Pull::Floating)?;
    let mut g9 = PinDriver::input(pins.gpio9)?;
    g9.set_pull(Pull::Up)?;

    let relay_main = RelayPin::new(LedcDriver::new(
        peripherals.ledc.channel1,
        &timer,
        pins.gpio6,
    )?);
    let relay_3_phase = RelayPin::new(LedcDriver::new(
        peripherals.ledc.channel2,
        &timer,
        pins.gpio18,
    )?);
    let analog = AdcDmaDriver::new(
        peripherals.adc1,
        pins.gpio3,
        pins.gpio1,
        pins.gpio0,
        pins.gpio4,
    )?;

    let control_pilot = LedcDriver::new(peripherals.ledc.channel0, &timer, pins.gpio2)?;
    let pilot_negative = InterruptPin::new(g9);
    let controller = Box::new(PhiEvseController::new(PhiEvsePeripherals {
        relay_main,
        relay_3_phase,
        v_sense: (g10, g19, g7),
        v_sense_3_phase: g5,
        analog,
        control_pilot,
        pilot_negative,
        watchdog: EspWatchdog,
    }));

    // Load configuration from NVS
    let config = PhiEvseConfig::load()?;
    println!("{config:#?}");

    // Build Wifi configurations
    let mut ap_config = AccessPointConfiguration {
        ssid: config.ap.ssid.as_str().try_into().unwrap(),
        ..Default::default()
    };
    if let Some(password) = config.ap.psk {
        ap_config.auth_method = embedded_svc::wifi::AuthMethod::WPA2Personal;
        ap_config.password = password.as_str().try_into().unwrap();
    }

    let mut needs_connect = false;
    let wifi_config = if let Some(sta) = config.sta {
        let mut sta_config = ClientConfiguration {
            ssid: sta.ssid.as_str().try_into().unwrap(),
            ..Default::default()
        };
        needs_connect = true;
        if let Some(password) = sta.psk {
            sta_config.auth_method = embedded_svc::wifi::AuthMethod::WPA2Personal;
            sta_config.password = password.as_str().try_into().unwrap();
        }

        embedded_svc::wifi::Configuration::Mixed(sta_config, ap_config)
    } else {
        embedded_svc::wifi::Configuration::AccessPoint(ap_config)
    };

    // Initialize Wifi
    let mut wifi_client_conf = NetifConfiguration::wifi_default_client();
    wifi_client_conf.ip_configuration = Some(Configuration::Client(
        embedded_svc::ipv4::ClientConfiguration::DHCP(DHCPClientSettings {
            hostname: Some(config.hostname.as_str().try_into().unwrap()),
        }),
    ));
    let wifi = Box::new(EspWifi::wrap_all(
        WifiDriver::new(peripherals.modem, EspSystemEventLoop::take()?, None)?,
        EspNetif::new_with_conf(&wifi_client_conf)?,
        EspNetif::new(NetifStack::Ap)?,
    )?);
    let wifi = Box::leak(wifi);
    wifi.set_configuration(&wifi_config)?;
    wifi.start()?;
    if needs_connect {
        wifi.connect()?;
    }

    // NTP
    let _ntp = EspSntp::new_default();

    // Initialize HTTP server
    println!("Starting HTTPd");
    let _h = httpd::start(
        ring_buffer,
        controller.status(),
        controller.control_channel(),
    )?;
    println!("HTTP running");

    if let Some(uri) = config.mqtt_uri {
        mqtt::start(&uri, controller.status(), controller.control_channel())?;
    }

    // Run
    Box::leak(controller).run();

    #[allow(unreachable_code)]
    Ok(())
}
