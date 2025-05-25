use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

use embedded_svc::mqtt::client::QoS;
use esp_idf_svc::mqtt::client::{EspMqttClient, EventPayload, MqttClientConfiguration};
use esp_idf_sys::EspError;
use phievse::{ControlMessage, PhiEvseStatus};

fn send_autodiscovery(mqtt: &mut EspMqttClient) -> Result<(), EspError> {
    mqtt.publish(
        "homeassistant/number/phievse/max_power/config",
        QoS::AtMostOnce,
        true,
        include_bytes!("max_power.json"),
    )?;
    mqtt.publish(
        "homeassistant/sensor/phievse/power/config",
        QoS::AtMostOnce,
        true,
        include_bytes!("power.json"),
    )?;
    mqtt.publish(
        "homeassistant/sensor/phievse/state/config",
        QoS::AtMostOnce,
        true,
        include_bytes!("state.json"),
    )?;

    Ok(())
}

fn send_state(mqtt: &mut EspMqttClient, status: Arc<Mutex<PhiEvseStatus>>) -> Result<(), EspError> {
    let status = status.lock().unwrap().clone();
    mqtt.publish(
        "phievse/state",
        QoS::AtMostOnce,
        false,
        &serde_json::to_vec(&status).unwrap(),
    )?;
    Ok(())
}

enum Event {
    Connected,
    SetMaxPower(u32),
}

pub fn start(
    mqtt_uri: &str,
    status: Arc<Mutex<PhiEvseStatus>>,
    control_channel: mpsc::Sender<ControlMessage>,
) -> Result<(), EspError> {
    let (tx, rx) = mpsc::channel();
    let mut mqtt = EspMqttClient::new_cb(
        mqtt_uri,
        &MqttClientConfiguration::default(),
        move |event| {
            let msg = match event.payload() {
                EventPayload::Connected(_) => Some(Event::Connected),
                EventPayload::Received { topic, data, .. } => {
                    if topic == Some("phievse/max_power") {
                        std::str::from_utf8(data)
                            .ok()
                            .and_then(|d| d.parse::<u32>().ok())
                            .map(Event::SetMaxPower)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(m) = msg {
                tx.send(m).unwrap();
            }
        },
    )?;
    thread::spawn(move || {
        let mut connected = false;
        loop {
            if let Ok(msg) = rx.try_recv() {
                match msg {
                    Event::Connected => {
                        mqtt.subscribe("phievse/max_power", QoS::AtMostOnce)
                            .unwrap_or_else(|_| {
                                log::warn!("Could not susbcribe");
                                0
                            });
                        send_autodiscovery(&mut mqtt)
                            .unwrap_or_else(|_| log::warn!("Could not send autodiscovery"));
                        connected = true;
                    }
                    Event::SetMaxPower(i) => {
                        control_channel
                            .send(ControlMessage::SetMaxPower(i))
                            .unwrap();
                        thread::sleep(Duration::from_millis(200)); // Give a bit of time for state to update
                    }
                }
            }

            // Send our state
            if connected {
                send_state(&mut mqtt, status.clone())
                    .unwrap_or_else(|_| log::warn!("Could not send state"));
            }

            thread::sleep(Duration::from_secs(5));
        }
    });

    Ok(())
}
