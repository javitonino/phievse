use esp_idf_sys::*;

use crate::watchdog::Watchdog;

pub struct EspWatchdog;

impl Watchdog for EspWatchdog {
    fn init(&self, timeout: std::time::Duration) {
        unsafe {
            esp_task_wdt_init(timeout.as_secs() as u32, true);
            esp_task_wdt_add(std::ptr::null_mut());
        }
    }

    fn reset(&self) {
        unsafe { esp_task_wdt_reset() };
    }

    fn stop(&self) {
        // I am not able to deinitialize the timer, so I just set a long timeout
        unsafe { esp_task_wdt_init(300, true) };
    }
}
