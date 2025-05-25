use enum_map::Enum;

#[derive(Debug, Enum, PartialEq)]
pub enum AdcChannel {
    CurrentL1,
    CurrentL2,
    CurrentL3,
    ControlPilot,
}

pub trait AdcSubscriber {
    fn subscribe(
        &mut self,
        receiver: impl FnMut(AdcChannel, &mut dyn Iterator<Item = i32>) + Send + 'static,
    );
}
