use crate::gpio::{AlarmInput, AlarmReceiver};
use esp_idf_hal::gpio::{Input, Pin, PinDriver};
use esp_idf_sys::*;

unsafe extern "C" fn irq_handler(data: *mut core::ffi::c_void) {
    unsafe {
        let data = &*(data as *mut HandlerData);
        // Recheck pin value (filter very short pulses)
        if gpio_get_level(data.pin) == 0 {
            gpio_intr_disable(data.pin);
            AlarmReceiver::alarm(&data.receiver);
        }
    }
}

struct HandlerData {
    receiver: AlarmReceiver,
    pin: gpio_num_t,
}

pub struct InterruptPin<'a, P: Pin> {
    pin: PinDriver<'a, P, esp_idf_hal::gpio::Input>,
    receiver: Option<HandlerData>,
}

impl<'a, P: Pin> InterruptPin<'a, P> {
    pub fn new(pin: PinDriver<'a, P, Input>) -> Self {
        Self {
            pin,
            receiver: None,
        }
    }
}

impl<'a, P: Pin> AlarmInput for InterruptPin<'a, P> {
    fn subscribe(&mut self, alarm: AlarmReceiver) {
        unsafe {
            let pin = self.pin.pin();
            let hdata = HandlerData {
                receiver: alarm,
                pin,
            };
            self.receiver = Some(hdata);
            gpio_isr_register(
                Some(irq_handler),
                self.receiver.as_ref().unwrap() as *const _ as *mut _,
                0,
                std::ptr::null_mut(),
            );
            gpio_set_intr_type(pin, gpio_int_type_t_GPIO_INTR_NEGEDGE);
        }
        self.arm();
    }

    fn arm(&self) {
        unsafe {
            gpio_intr_enable(self.pin.pin());
        }
    }

    fn is_high(&self) -> bool {
        self.pin.is_high()
    }
}
