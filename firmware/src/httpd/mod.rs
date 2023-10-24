use std::{sync::{Arc, Mutex, mpsc}, thread, time::Duration};

use askama::Template;
use embedded_svc::{http::server::*, io::Write, utils::io::try_read_full};
use esp_idf_svc::{http::server::*, errors::EspIOError};
use phievse::{logger::StringRingBuffer, PhiEvseStatus, ControlMessage};

mod ota;
mod config;

#[derive(Template)]
#[template(path = "log.html")]
struct LogTemplate<'a, const S: usize> {
    page: &'a str,
    messages: &'a StringRingBuffer<S>,
}

#[derive(Template)]
#[template(path = "status.html")]
struct StatusTemplate<'a> {
    page: &'a str,
    status: &'a PhiEvseStatus,
}

fn milligram(req: Request<&mut EspHttpConnection>) -> HandlerResult {
    let mut response = req.into_response(200, Some("OK"), &[("Content-Type", "text/css")])?;
    response.write_all(include_str!("../../templates/assets/milligram.min.css").as_bytes())?;
    Ok(())
}

fn ota_restart(req: Request<&mut EspHttpConnection>) -> HandlerResult {
    thread::spawn(|| {
        thread::sleep(Duration::from_secs(1));
        unsafe { esp_idf_sys::esp_restart() };
    });
    req.into_ok_response()?;
    Ok(())
}

fn redirect(req: Request<&mut EspHttpConnection>, to: &str) -> HandlerResult {
    req.into_response(302, Some("Found"), &[("Location", to)])?;

    Ok(())
}

pub fn start<const S: usize>(log_buffer: Arc<Mutex<Box<StringRingBuffer<S>>>>, status: Arc<Mutex<PhiEvseStatus>>, control_channel: mpsc::Sender<ControlMessage>) -> Result<EspHttpServer, EspIOError> {
    let mut httpd = EspHttpServer::new(&Configuration { ..Default::default() })?;

    // Static
    httpd.fn_handler("/milligram.min.css", Method::Get, milligram)?;

    // Status
    let st = status.clone();
    httpd.fn_handler("/", Method::Get, move |req| {
        let mut response = req.into_ok_response()?;
        response.write_all(StatusTemplate { status: &*st.lock()?, page: "status" }.render()?.as_bytes())?;
        Ok(())
    })?;

    let cc = control_channel.clone();
    httpd.fn_handler("/power", Method::Post, move |mut req| {
        let mut data = [0u8; 512];
        let len = try_read_full(&mut req, &mut data)?;
        let form = form_urlencoded::parse(&data[..len]);
        for (key, value) in form {
            if key == "max_power" {
                cc.send(ControlMessage::SetMaxPower(value.parse()?))?;        
            }
        }

        redirect(req, "/")
    })?;

    httpd.fn_handler("/shutdown", Method::Post, move |req| {
        control_channel.send(ControlMessage::Shutdown)?;

        redirect(req, "/")
    })?;

    httpd.fn_handler("/restart", Method::Post, ota_restart)?;

    // Logs
    httpd.fn_handler("/log", Method::Get, move |req| {
        let mut response = req.into_ok_response()?;
        response.write_all(LogTemplate { messages: &*log_buffer.lock()?, page: "logs" }.render()?.as_bytes())?;
        Ok(())
    })?;

    // OTA
    ota::register(&mut httpd, status)?;

    // Config
    config::register(&mut httpd)?;

    Ok(httpd)
}
