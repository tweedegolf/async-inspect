#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_futures::{join::join_array, select::select_array};
use embassy_nrf::{
    bind_interrupts,
    gpio::{Input, Level, Output, OutputDrive, Pull},
    peripherals, rng,
};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    RNG => rng::InterruptHandler<peripherals::RNG>;
});

static mut SEQUENCE: [u8; 1024] = [0u8; 1024];

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());

    let rng = rng::Rng::new(p.RNG, Irqs);

    let mut leds = [
        Output::new(p.P0_13, Level::Low, OutputDrive::Standard),
        Output::new(p.P0_14, Level::Low, OutputDrive::Standard),
        Output::new(p.P0_15, Level::Low, OutputDrive::Standard),
        Output::new(p.P0_16, Level::Low, OutputDrive::Standard),
    ];
    let buttons = [
        Input::new(p.P0_11, Pull::Up),
        Input::new(p.P0_12, Pull::Up),
        Input::new(p.P0_24, Pull::Up),
        Input::new(p.P0_25, Pull::Up),
    ];

    leds.iter_mut().for_each(|l| l.set_high());

    let sequence = unsafe { &mut *&raw mut SEQUENCE };

    _spawner.must_spawn(game(rng, leds, buttons, sequence));
}

#[embassy_executor::task]
async fn game(
    mut rng: rng::Rng<'static, peripherals::RNG, embassy_nrf::mode::Async>,
    mut leds: [Output<'static>; 4],
    mut buttons: [Input<'static>; 4],
    sequence: &'static mut [u8],
) -> ! {
    let mut len;

    loop {
        len = 0;
        'game: loop {
            rng.fill_bytes(&mut sequence[len..=len]).await;
            sequence[len] %= 4;
            len += 1;

            for i in &sequence[..len] {
                leds[*i as usize].set_low();
                Timer::after_millis(200).await;
                leds[*i as usize].set_high();
                Timer::after_millis(200).await;
            }

            for i in &sequence[..len] {
                let b_i = select_array(buttons.each_mut().map(|b| b.wait_for_low()).as_mut_slice())
                    .await
                    .1;

                leds[*i as usize].set_low();
                if b_i != *i as usize {
                    break 'game;
                }
                join_array(buttons.each_mut().map(|b| b.wait_for_high())).await;
                leds[*i as usize].set_high();
            }

            Timer::after_millis(600).await;
        }

        Timer::after_millis(300).await;
        for _ in 0..4 {
            leds.iter_mut().for_each(|l| l.set_low());
            Timer::after_millis(300).await;
            leds.iter_mut().for_each(|l| l.set_high());
            Timer::after_millis(300).await;
        }
    }
}
