use std::error::Error;

use crate::led::LedDriver;
use esp_idf_sys::*;

const fn rmt_item(duration0: u32, level0: u32, duration1: u32, level1: u32) -> rmt_item32_t {
    rmt_item32_t {
        __bindgen_anon_1: rmt_item32_t__bindgen_ty_1 {
            val: duration0 + (level0 << 15) + (duration1 << 16) + (level1 << 23),
        },
    }
}

/// A driver for RGB LEDs using the RMT controller
pub struct RmtDriver {
    channel: rmt_channel_t,
}

impl Drop for RmtDriver {
    fn drop(&mut self) {
        esp!(unsafe { rmt_driver_uninstall(self.channel) }).expect("Failure uninstalling RMT driver");
    }
}

impl RmtDriver {
    pub fn init(pin: i32, channel: Option<rmt_channel_t>) -> Result<Self, EspError> {
        let channel = channel.unwrap_or(rmt_channel_t_RMT_CHANNEL_0);
        let config = rmt_config_t {
            rmt_mode: rmt_mode_t_RMT_MODE_TX,
            channel,
            gpio_num: pin,
            clk_div: 4,
            mem_block_num: 1,
            flags: 0,
            __bindgen_anon_1: rmt_config_t__bindgen_ty_1 {
                tx_config: rmt_tx_config_t {
                    carrier_freq_hz: 0,
                    carrier_level: rmt_carrier_level_t_RMT_CARRIER_LEVEL_HIGH,
                    idle_level: rmt_idle_level_t_RMT_IDLE_LEVEL_LOW,
                    carrier_duty_percent: 0,
                    carrier_en: false,
                    idle_output_en: true,
                    loop_en: false,
                    loop_count: 0,
                },
            },
        };

        esp!(unsafe { rmt_config(&config) })?;
        esp!(unsafe { rmt_driver_install(channel, 0, 0) })?;

        Ok(Self { channel })
    }
}

impl LedDriver for RmtDriver {
    fn set_rgb(&self, r: u8, g: u8, b: u8) -> anyhow::Result<(), Box<dyn Error>> {
        const B0: rmt_item32_t = rmt_item(7, 1, 18, 0); // 3.5us high, 9us low (total > 1.2us)
        const B1: rmt_item32_t = rmt_item(18, 1, 7, 0); // 9us high, 3.5us low (total > 1.2us)
        const RST: rmt_item32_t = rmt_item(800, 0, 800, 0); //80us low

        let data: [rmt_item32_t; 25] = [
            if (g & 0x80) > 0 { B1 } else { B0 },
            if (g & 0x40) > 0 { B1 } else { B0 },
            if (g & 0x20) > 0 { B1 } else { B0 },
            if (g & 0x10) > 0 { B1 } else { B0 },
            if (g & 0x08) > 0 { B1 } else { B0 },
            if (g & 0x04) > 0 { B1 } else { B0 },
            if (g & 0x02) > 0 { B1 } else { B0 },
            if (g & 0x01) > 0 { B1 } else { B0 },
            if (r & 0x80) > 0 { B1 } else { B0 },
            if (r & 0x40) > 0 { B1 } else { B0 },
            if (r & 0x20) > 0 { B1 } else { B0 },
            if (r & 0x10) > 0 { B1 } else { B0 },
            if (r & 0x08) > 0 { B1 } else { B0 },
            if (r & 0x04) > 0 { B1 } else { B0 },
            if (r & 0x02) > 0 { B1 } else { B0 },
            if (r & 0x01) > 0 { B1 } else { B0 },
            if (b & 0x80) > 0 { B1 } else { B0 },
            if (b & 0x40) > 0 { B1 } else { B0 },
            if (b & 0x20) > 0 { B1 } else { B0 },
            if (b & 0x10) > 0 { B1 } else { B0 },
            if (b & 0x08) > 0 { B1 } else { B0 },
            if (b & 0x04) > 0 { B1 } else { B0 },
            if (b & 0x02) > 0 { B1 } else { B0 },
            if (b & 0x01) > 0 { B1 } else { B0 },
            RST,
        ];

        esp!(unsafe { rmt_write_items(self.channel, &data[0], 25, true) })?;
        Ok(())
    }
}
