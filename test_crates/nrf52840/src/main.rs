#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_nrf::{
    Peri,
    gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull},
};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::task(pool_size = 4)]
async fn on_button(led: Peri<'static, AnyPin>, button: Peri<'static, AnyPin>) {
    let mut led = Output::new(led, Level::Low, OutputDrive::Standard);
    let mut button = Input::new(button, Pull::Up);

    loop {
        button.wait_for_high().await;
        led.set_high();

        button.wait_for_low().await;
        led.set_low();
    }
}

// Main is itself an async task as well.
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Initialize the embassy-nrf HAL.
    let p = embassy_nrf::init(Default::default());

    let b1 = on_button(p.P0_13.into(), p.P0_11.into());
    let b2 = on_button(p.P0_14.into(), p.P0_12.into());
    let b3 = on_button(p.P0_15.into(), p.P0_24.into());
    let b4 = on_button(p.P0_16.into(), p.P0_25.into());
    // Spawned tasks run in the background, concurrently.
    spawner.spawn(b1).unwrap();
    spawner.spawn(b2).unwrap();
    spawner.spawn(b3).unwrap();
    spawner.spawn(b4).unwrap();

    info!("All threads spawned");
}
