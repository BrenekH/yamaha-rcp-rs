use std::error::Error;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time;

#[tokio::main]
async fn main() {
    let mut mixer = Mixer::new("192.168.0.128:49280").await.unwrap();
    mixer.fade(1, 10_000, -32768, 500).await.unwrap();
}

struct Mixer {
    stream: TcpStream,
}

impl Mixer {
    async fn new(addr: &str) -> Result<Self, Box<dyn Error>> {
        Ok(Mixer {
            stream: TcpStream::connect(addr).await?,
        })
    }

    async fn fade(
        &mut self,
        channel: u16,
        initial_value: i32,
        final_value: i32,
        duration_ms: u64,
    ) -> std::io::Result<()> {
        let num_steps: u64 = duration_ms / 50;
        let step_delta: i32 = (final_value - initial_value) / (num_steps as i32);

        let mut interval = time::interval(time::Duration::from_millis(50));
        let mut current_value = initial_value;
        let mut response_buf = [0; 256];

        for _i in 0..num_steps {
            interval.tick().await;

            self.stream
                .write_all(
                    format!("set MIXER:Current/InCh/Fader/Level {channel} 0 {current_value}\n")
                        .as_bytes(),
                )
                .await?;
            println!("Set channel {channel} to {current_value}");

            self.stream.read(&mut response_buf).await?;
            println!("{}", std::str::from_utf8(&response_buf).unwrap());

            current_value += step_delta;
        }

        self.stream
            .write(
                format!("set MIXER:Current/InCh/Fader/Level {channel} 0 {final_value}\n")
                    .as_bytes(),
            )
            .await?;
        println!("Set channel {channel} to {final_value}");

        self.stream.read(&mut response_buf).await?;
        println!("{}", std::str::from_utf8(&response_buf).unwrap());

        Ok(())
    }
}
