use std::io::prelude::*;
use std::net::TcpStream;
use tokio::time;

async fn fade(
    addr: &str,
    channel: u16,
    initial_value: i32,
    final_value: i32,
    duration_ms: u64,
) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(addr)?;
    println!("stream connected");

    let num_steps: u64 = duration_ms / 50;
    let step_delta: i32 = (final_value - initial_value) / (num_steps as i32);

    let mut interval = time::interval(time::Duration::from_millis(50));
    let mut current_value = initial_value;
    let mut response_buf = [0; 256];

    for _i in 0..num_steps {
        interval.tick().await;

        stream.write(
            format!("set MIXER:Current/InCh/Fader/Level {channel} 0 {current_value}\n").as_bytes(),
        )?;
        println!("Set channel {channel} to {current_value}");

        stream.read(&mut response_buf)?;
        println!("{}", std::str::from_utf8(&response_buf).unwrap());

        current_value += step_delta;
    }

    stream.write(
        format!("set MIXER:Current/InCh/Fader/Level {channel} 0 {final_value}\n").as_bytes(),
    )?;
    println!("Set channel {channel} to {final_value}");

    stream.read(&mut response_buf)?;
    println!("{}", std::str::from_utf8(&response_buf).unwrap());

    Ok(())
}

#[tokio::main]
async fn main() {
    fade("192.168.0.128:49280", 1, 10_000, -32768, 500)
        .await
        .unwrap();
}
