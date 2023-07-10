// We use an underscore to make our decibel values more readable, which
// Clippy by default does not agree with.
#![allow(clippy::inconsistent_digit_grouping)]

use futures::future;
use tokio::time;

use yamaha_rcp_rs::{LabelColor, TFMixer};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mixer = TFMixer::new("192.168.0.128:49280").await?;
    println!("Connected to mixer!");

    println!("{:?}", mixer.color(0).await?);
    mixer.set_color(1, LabelColor::Blue).await?;

    println!("{}", mixer.label(0).await?);
    mixer.set_label(1, "CHAN 2").await?;

    mixer.fader_level(1).await?;

    let chan1_fader = mixer.fade(1, 10_00, -40_00, 3_000);
    let chan2_fader = mixer.fade(2, -40_00, 10_00, 3_000);
    let results = future::join(chan1_fader, chan2_fader).await;
    results.0?;
    results.1?;
    mixer.set_fader_level(1, -138_00).await?;

    time::sleep(time::Duration::from_secs(3)).await;

    let chan1_fader = mixer.fade(1, -40_00, 10_00, 3_000);
    let chan2_fader = mixer.fade(2, 10_00, -40_00, 3_000);
    let results = future::join(chan1_fader, chan2_fader).await;
    results.0?;
    results.1?;
    mixer.set_fader_level(2, -138_00).await?;

    mixer.muted(0).await?;
    mixer.set_muted(0, true).await?;

    time::sleep(time::Duration::from_secs(1)).await;
    println!("Starting stress pattern");

    let mut async_tasks = vec![];
    for i in 0..=15 {
        let tf1 = mixer.clone();
        async_tasks.push(async move {
            tf1.set_fader_level(i, -40_00).await?;
            time::sleep(time::Duration::from_millis((i * 500).into())).await;
            tf1.fade(i, -40_00, 10_00, 3000).await?;
            tf1.fade(i, 10_00, -40_00, 3000).await?;
            tf1.set_fader_level(i, -32800).await?;
            Ok::<(), yamaha_rcp_rs::Error>(())
        });
    }

    let results = future::join_all(async_tasks).await;
    for result in results {
        result?;
    }

    Ok(())
}
