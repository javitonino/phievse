use std::sync::{Mutex, Arc};

use askama::Template;
use embedded_svc::http::Method;
use embedded_svc::http::server::{Request, HandlerResult};
use embedded_svc::io::Write;
use embedded_svc::ota::*;
use esp_idf_svc::http::server::{EspHttpServer, EspHttpConnection};
use esp_idf_svc::ota::EspOta;
use esp_idf_sys::*;
use phievse::{PhiEvseState, PhiEvseStatus};

struct PartitionInfo<'a> {
    label: &'a str,
    version: String,
    date: String,
    time: String,
    state: String,
    boot: bool,
    running: bool,
}

#[derive(Template)]
#[template(path = "ota_info.html")]
struct OtaInfoTemplate<'a> {
    page: &'a str,
    partitions: Vec<PartitionInfo<'a>>,
}

fn str_from_c(cstr: &[i8]) -> &str {
    unsafe { std::ffi::CStr::from_ptr(cstr.as_ptr()) }
        .to_str()
        .unwrap_or("")
}

fn redirect(req: Request<&mut EspHttpConnection>, to: &str) -> HandlerResult {
    req.into_response(302, Some("Found"), &[("Location", to)])?;

    Ok(())
}

fn ota_info(req: Request<&mut EspHttpConnection>) -> HandlerResult {
    let mut iterator = unsafe {
        esp_idf_sys::esp_partition_find(
            esp_idf_sys::esp_partition_type_t_ESP_PARTITION_TYPE_APP,
            esp_idf_sys::esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_ANY,
            std::ptr::null::<i8>(),
        )
    };

    let mut partitions: Vec<PartitionInfo> = vec![];

    let boot = unsafe { esp_ota_get_boot_partition() };
    let running = unsafe { esp_ota_get_running_partition() };

    while !iterator.is_null() {
        let partition_ptr = unsafe { esp_partition_get(iterator) };

        let mut app_desc = esp_app_desc_t { ..Default::default() };
        unsafe { esp_ota_get_partition_description(partition_ptr, &mut app_desc) };

        let mut state: esp_ota_img_states_t = 0;
        let state = match esp!(unsafe { esp_ota_get_state_partition(partition_ptr, &mut state) }) {
            Err(e) => e.to_string(),
            #[allow(non_upper_case_globals)]
            Ok(()) => match state {
                esp_ota_img_states_t_ESP_OTA_IMG_VALID => "valid".to_string(),
                esp_ota_img_states_t_ESP_OTA_IMG_UNDEFINED => "undefined".to_string(),
                esp_ota_img_states_t_ESP_OTA_IMG_INVALID => "invalid".to_string(),
                esp_ota_img_states_t_ESP_OTA_IMG_ABORTED => "aborted".to_string(),
                esp_ota_img_states_t_ESP_OTA_IMG_NEW => "new".to_string(),
                esp_ota_img_states_t_ESP_OTA_IMG_PENDING_VERIFY => "pending-verify".to_string(),
                _ => "unknown".to_string(),
            },
        };

        let partition = unsafe { partition_ptr.as_ref().unwrap() };
        partitions.push(PartitionInfo {
            label: str_from_c(&partition.label),
            version: str_from_c(&app_desc.version).to_string(),
            date: str_from_c(&app_desc.date).to_string(),
            time: str_from_c(&app_desc.time).to_string(),
            state,
            boot: boot == partition_ptr,
            running: running == partition_ptr,
        });

        iterator = unsafe { esp_partition_next(iterator) };
    }

    let mut response = req.into_ok_response()?;
    response.write_all(OtaInfoTemplate { partitions, page: "ota" }.render()?.as_bytes())?;
    Ok(())
}

fn ota_complete(req: Request<&mut EspHttpConnection>) -> HandlerResult {
    unsafe { esp_idf_sys::esp_ota_mark_app_valid_cancel_rollback() };
    redirect(req, "/ota")
}

fn ota_update(mut req: Request<&mut EspHttpConnection>) -> HandlerResult {
    // Do the OTA
    let mut ota = EspOta::new()?;
    let update = ota.initiate_update()?;
    update.update(&mut req, |_, _| {})?;
    req.into_ok_response()?;
    Ok(())
}

fn ota_set_boot(mut req: Request<&mut EspHttpConnection>) -> HandlerResult {
    let mut buf = [0u8; 128];
    let len = req.read(&mut buf)?;
    let mut form = form_urlencoded::parse(&buf[..len]);
    if let Some((_, label)) = form.find(|x| x.0 == "label") {
        let partition = unsafe {
            esp_idf_sys::esp_partition_find_first(
                esp_idf_sys::esp_partition_type_t_ESP_PARTITION_TYPE_APP,
                esp_idf_sys::esp_partition_subtype_t_ESP_PARTITION_SUBTYPE_ANY,
                label.as_ptr() as *const _,
            )
        };
        esp!(unsafe { esp_ota_set_boot_partition(partition) })?;
    }

    redirect(req, "/ota")
}

pub fn register(httpd: &mut EspHttpServer, status: Arc<Mutex<PhiEvseStatus>>) -> Result<(), EspError> {
    httpd.fn_handler("/ota", Method::Get, ota_info)?;
    httpd.fn_handler("/ota/verify", Method::Post, ota_complete)?;
    httpd.fn_handler("/ota/update", Method::Post, move |req| {
        // Check that we are not charging
        if status.lock().unwrap().state != PhiEvseState::Shutdown {
            let mut res = req.into_status_response(400)?;
            res.write_all("Please shutdown before OTA".as_bytes())?;
            Ok(())
        } else {
            ota_update(req)
        }
    })?;
    httpd.fn_handler("/ota/boot", Method::Post, ota_set_boot)?;

    Ok(())
}
