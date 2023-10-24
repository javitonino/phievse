pub trait Watchdog {
    fn init(&self, timeout: std::time::Duration);
    fn reset(&self);
    fn stop(&self);
}
