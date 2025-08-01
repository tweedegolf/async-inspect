#![no_std]
#![no_main]

use cortex_m_rt::entry;
use defmt::info;
use {defmt_rtt as _, panic_probe as _};

#[entry]
fn main() -> ! {
    loop {
        info!("Msg1");
        info!("Msg2");
    }
}
