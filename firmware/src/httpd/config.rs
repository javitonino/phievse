use askama::Template;
use embedded_svc::http::server::Request;
use embedded_svc::http::Method;
use embedded_svc::io::Write;
use embedded_svc::utils::io::try_read_full;
use esp_idf_svc::http::server::{EspHttpConnection, EspHttpServer};
use esp_idf_sys::*;

use crate::config::*;

#[derive(Template)]
#[template(path = "config.html")]
struct ConfigTemplate<'a> {
    page: &'a str,
    message: Option<&'a str>,
    config: &'a PhiEvseConfig,
}

fn show(req: Request<&mut EspHttpConnection>, message: Option<&str>) -> Result<(), anyhow::Error> {
    let config = &PhiEvseConfig::load()?;

    let mut response = req.into_ok_response()?;
    response.write_all(
        ConfigTemplate {
            message,
            config,
            page: "config",
        }
        .render()?
        .as_bytes(),
    )?;
    Ok(())
}

fn save(mut req: Request<&mut EspHttpConnection>) -> Result<(), anyhow::Error> {
    let mut data = [0u8; 512];
    let len = try_read_full(&mut req, &mut data).map_err(|e| e.0)?;
    let form = form_urlencoded::parse(&data[..len]);

    let mut hostname: Option<String> = None;
    let mut sta_ssid: Option<String> = None;
    let mut sta_psk: Option<String> = None;
    let mut ap_ssid: Option<String> = None;
    let mut ap_psk: Option<String> = None;
    let mut mqtt_uri: Option<String> = None;

    for (key, value) in form {
        if value.is_empty() {
            continue;
        }
        match key.as_ref() {
            "hostname" => hostname = Some(value.to_string()),
            "sta.ssid" => sta_ssid = Some(value.to_string()),
            "sta.psk" => sta_psk = Some(value.to_string()),
            "ap.ssid" => ap_ssid = Some(value.to_string()),
            "ap.psk" => ap_psk = Some(value.to_string()),
            "mqtt.uri" => mqtt_uri = Some(value.to_string()),
            _ => log::warn!("Unknown config key: {key}"),
        }
    }
    if hostname.is_none() || ap_ssid.is_none() {
        return show(req, Some("Hostname and AP SSID are mandatory"));
    }

    let config = PhiEvseConfig {
        hostname: hostname.unwrap(),
        ap: WifiConfig {
            ssid: ap_ssid.unwrap(),
            psk: ap_psk,
        },
        sta: sta_ssid.map(|ssid| WifiConfig { ssid, psk: sta_psk }),
        mqtt_uri,
    };

    if let Err(e) = config.save() {
        log::warn!("Error saving config {e}");
        show(req, Some("Error saving configuration"))
    } else {
        show(req, Some("Configuration saved"))
    }
}

pub fn register(httpd: &mut EspHttpServer) -> Result<(), EspError> {
    httpd.fn_handler("/config", Method::Get, |req| show(req, None))?;
    httpd.fn_handler("/config", Method::Post, save)?;

    Ok(())
}
