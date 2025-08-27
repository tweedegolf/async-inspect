#![no_std]
#![no_main]

use defmt::info;
use embassy_executor::Spawner;
use embassy_futures::{join::join, select::select};
use embassy_nrf::{
    Peri,
    gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull},
};
use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::task(pool_size = 1)]
async fn wait_on_both(
    led: Peri<'static, AnyPin>,
    button1: Peri<'static, AnyPin>,
    button2: Peri<'static, AnyPin>,
) {
    let mut led = Output::new(led, Level::Low, OutputDrive::Standard);
    let mut button1 = Input::new(button1, Pull::Up);
    let mut button2 = Input::new(button2, Pull::Up);

    loop {
        join(button1.wait_for_high(), button2.wait_for_high()).await;
        led.set_high();

        join(button1.wait_for_low(), button2.wait_for_low()).await;
        led.set_low();
    }
}

#[embassy_executor::task(pool_size = 1)]
async fn wait_on_one(
    led1: Peri<'static, AnyPin>,
    led2: Peri<'static, AnyPin>,
    button1: Peri<'static, AnyPin>,
    button2: Peri<'static, AnyPin>,
) {
    let mut led1 = Output::new(led1, Level::Low, OutputDrive::Standard);
    let mut led2 = Output::new(led2, Level::Low, OutputDrive::Standard);
    let mut button1 = Input::new(button1, Pull::Up);
    let mut button2 = Input::new(button2, Pull::Up);

    loop {
        led1.set_high();
        led2.set_high();

        let r = select(button1.wait_for_low(), button2.wait_for_low()).await;
        match r {
            embassy_futures::select::Either::First(_) => {
                led1.set_low();
                button1.wait_for_high().await;
            }
            embassy_futures::select::Either::Second(_) => {
                led2.set_low();
                button2.wait_for_high().await;
            }
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());

    let b1 = wait_on_both(p.P0_13.into(), p.P0_11.into(), p.P0_12.into());
    let b2 = wait_on_one(
        p.P0_15.into(),
        p.P0_16.into(),
        p.P0_24.into(),
        p.P0_25.into(),
    );

    spawner.spawn(b1).unwrap();
    spawner.spawn(b2).unwrap();

    info!("All threads spawned");
}
