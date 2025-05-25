use esp_idf_sys::*;

use crate::watchdog::Watchdog;

pub struct EspWatchdog;

impl Watchdog for EspWatchdog {
    fn init(&self, timeout: std::time::Duration) {
        unsafe {
            let config = esp_task_wdt_config_t {
                timeout_ms: timeout.as_millis() as u32,
                idle_core_mask: 1,
                trigger_panic: true,
            };
            esp_task_wdt_init(&config);
            esp_task_wdt_add(std::ptr::null_mut());
        }
    }

    fn reset(&self) {
        unsafe { esp_task_wdt_reset() };
    }

    fn stop(&self) {
        unsafe {
            esp_task_wdt_delete(std::ptr::null_mut());
            esp_task_wdt_deinit();
        };
    }
}
