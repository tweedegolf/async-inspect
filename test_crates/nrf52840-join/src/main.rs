#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::join::join4;
use embassy_nrf::{
    Peri,
    gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull},
};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::task(pool_size = 1)]
async fn wait_on_all(
    led: Peri<'static, AnyPin>,
    button1: Peri<'static, AnyPin>,
    button2: Peri<'static, AnyPin>,
    button3: Peri<'static, AnyPin>,
    button4: Peri<'static, AnyPin>,
) {
    let mut led = Output::new(led, Level::Low, OutputDrive::Standard);
    let mut button1 = Input::new(button1, Pull::Up);
    let mut button2 = Input::new(button2, Pull::Up);
    let mut button3 = Input::new(button3, Pull::Up);
    let mut button4 = Input::new(button4, Pull::Up);

    loop {
        join4(
            button1.wait_for_low(),
            button2.wait_for_low(),
            button3.wait_for_low(),
            button4.wait_for_low(),
        ).await;
        led.set_low();

        join4(
            button1.wait_for_high(),
            button2.wait_for_high(),
            button3.wait_for_high(),
            button4.wait_for_high(),
        ).await;
        led.set_high();
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());

    let b1 = wait_on_all(
        p.P0_13.into(),
        p.P0_11.into(),
        p.P0_12.into(),
        p.P0_24.into(),
        p.P0_25.into(),
    );

    spawner.spawn(b1).unwrap();

    info!("All threads spawned");
}
