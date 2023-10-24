// Necessary because of this issue: https://github.com/rust-lang/cargo/issues/9641
use std::env;
fn main() -> anyhow::Result<()> {
    if env::var("CARGO_CFG_TARGET_ARCH") == Ok("riscv32".into()) {
        embuild::build::CfgArgs::output_propagated("ESP_IDF")?;
        embuild::build::LinkArgs::output_propagated("ESP_IDF")?;
    }
    Ok(())
}
