use esp_idf_svc::nvs::{EspDefaultNvs, EspDefaultNvsPartition};
use esp_idf_sys::EspError;

#[derive(Debug)]
pub struct PhiEvseConfig {
    pub hostname: String,
    pub sta: Option<WifiConfig>,
    pub ap: WifiConfig,
    pub mqtt_uri: Option<String>,
}

#[derive(Debug)]
pub struct WifiConfig {
    pub ssid: String,
    pub psk: Option<String>,
}

impl PhiEvseConfig {
    pub fn load() -> Result<Self, anyhow::Error> {
        let nvs = EspDefaultNvs::new(EspDefaultNvsPartition::take()?, "phievse", true)?;

        Ok(Self {
            hostname: get_string(&nvs, "hostname")?.unwrap_or("phievse".into()),
            sta: WifiConfig::load(&nvs, "sta")?,
            ap: WifiConfig::load(&nvs, "ap")?.unwrap_or(WifiConfig { ssid: "phievse".into(), psk: None }),
            mqtt_uri: get_string(&nvs, "mqtt.uri")?,
        })
    }

    pub fn save(&self) -> Result<(), anyhow::Error> {
        let mut nvs = EspDefaultNvs::new(EspDefaultNvsPartition::take()?, "phievse", true)?;

        set_string(&mut nvs, "hostname", Some(&self.hostname))?;
        if let Some(sta) = &self.sta {
            sta.save(&mut nvs, "sta")?;
        } else {
            WifiConfig::remove(&mut nvs, "sta")?;
        }
        self.ap.save(&mut nvs, "ap")?;
        set_string(&mut nvs, "mqtt.uri", self.mqtt_uri.as_ref())?;

        Ok(())
    }
}

impl WifiConfig {
    fn load(nvs: &EspDefaultNvs, prefix: &str) -> Result<Option<Self>, anyhow::Error> {
        let ssid = get_string(nvs, &format!("{prefix}.ssid"))?;
        if let Some(ssid) = ssid {
            Ok(Some(WifiConfig {
                ssid,
                psk: get_string(nvs, &format!("{prefix}.psk"))?
            }))
        } else {
            Ok(None)
        }
    }

    fn save(&self, nvs: &mut EspDefaultNvs, prefix: &str) -> Result<(), anyhow::Error> {
        set_string(nvs, &format!("{prefix}.ssid"), Some(&self.ssid))?;
        set_string(nvs, &format!("{prefix}.psk"), self.psk.as_ref())?;

        Ok(())
    }

    fn remove(nvs: &mut EspDefaultNvs, prefix: &str) -> Result<(), anyhow::Error> {
        set_string(nvs, &format!("{prefix}.ssid"), None)?;
        set_string(nvs, &format!("{prefix}.psk"), None)?;

        Ok(())
    }
}

fn set_string(nvs: &mut EspDefaultNvs, key: &str, value: Option<&String>) -> Result<(), EspError> {
    if let Some(v) = value {
        nvs.set_str(key, v)
    } else {
        nvs.remove(key).map(|_| ())
    }
}

fn get_string(nvs: &EspDefaultNvs, key: &str) -> Result<Option<String>, anyhow::Error> {
    let len = nvs.str_len(key)?;
    if let Some(len) = len {
        let mut buf = vec![0u8; len];
        Ok(nvs.get_str(key, &mut buf)?.map(|s| s[..len-1].into()))
    } else {
        Ok(None)
    }
}