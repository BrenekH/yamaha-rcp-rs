// We use an underscore to make our decibel values more readable, which
// Clippy by default does not agree with.
#![allow(clippy::inconsistent_digit_grouping)]

use futures::future;
use tokio::time;

use yamaha_rcp_rs::{LabelColor, Mixer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut mixer = Mixer::new("192.168.0.128:49280").await?;
    let mut mixer2 = Mixer::new("192.168.0.128:49280").await?;
    // mixer.set_debug(true);
    println!("Connected to mixer!");

    println!("{:?}", mixer.color(0).await?);
    mixer.set_color(1, LabelColor::Blue).await?;

    println!("{}", mixer.label(0).await?);
    mixer.set_label(1, "CHAN 2").await?;

    mixer.fader_level(1).await?;
    // mixer.fade(1, 10_00, -138_00, 10_000).await?;
    // time::sleep(time::Duration::from_secs(1)).await;
    // mixer.fade(1, -138_00, 10_00, 1_000).await?;

    let chan1_fader = mixer.fade(1, 10_00, -40_00, 3_000);
    let chan2_fader = mixer2.fade(2, -40_00, 10_00, 3_000);
    let results = future::join(chan1_fader, chan2_fader).await;
    results.0?;
    results.1?;
    mixer.set_fader_level(1, -138_00).await?;

    time::sleep(time::Duration::from_secs(3)).await;

    let chan1_fader = mixer.fade(1, -40_00, 10_00, 3_000);
    let chan2_fader = mixer2.fade(2, 10_00, -40_00, 3_000);
    let results = future::join(chan1_fader, chan2_fader).await;
    results.0?;
    results.1?;
    mixer2.set_fader_level(2, -138_00).await?;

    mixer.muted(0).await?;
    mixer.set_muted(0, true).await?;

    Ok(())
}
