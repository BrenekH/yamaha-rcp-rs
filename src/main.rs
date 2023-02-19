use std::error::Error;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time;

#[tokio::main]
async fn main() {
    let mut mixer = Mixer::new("192.168.0.128:49280").await.unwrap();
    println!("Connected to mixer!");

    mixer.fader_level(1).await.unwrap();
    mixer.fade(1, 10_00, -138_00, 1_000).await.unwrap();

    mixer.muted(0).await.unwrap();
    mixer.set_muted(0, true).await.unwrap();
}

struct Mixer {
    stream: TcpStream,
    max_fader_val: i32,
    min_fader_val: i32,
    neg_inf_val: i32,
}

impl Mixer {
    async fn new(addr: &str) -> Result<Self, Box<dyn Error>> {
        Ok(Mixer {
            stream: TcpStream::connect(addr).await?,
            max_fader_val: 10_00,
            min_fader_val: -138_00,
            neg_inf_val: -327_68,
        })
    }

    async fn send_command(&mut self, cmd: String) -> Result<String, Box<dyn Error>> {
        self.stream.write_all(cmd.as_bytes()).await?;

        let mut response_buf = [0; 256];
        self.stream.read(&mut response_buf).await?;

        let result = std::str::from_utf8(&response_buf).unwrap();
        Ok(result.to_owned())
    }

    async fn fader_level(&mut self, channel: u16) -> Result<i32, Box<dyn Error>> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Fader/Level {channel} 0\n"))
            .await?;
        println!("{response}");
        Ok(0)
    }

    async fn set_fader_level(&mut self, channel: u16, value: i32) -> Result<(), Box<dyn Error>> {
        let response = self
            .send_command(format!(
                "set MIXER:Current/InCh/Fader/Level {channel} 0 {value}\n"
            ))
            .await?;
        println!("{response}");
        Ok(())
    }

    async fn muted(&mut self, channel: u16) -> Result<bool, Box<dyn Error>> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Fader/On {channel} 0\n"))
            .await?;
        println!("{response}");
        Ok(false)
    }

    async fn set_muted(&mut self, channel: u16, muted: bool) -> Result<(), Box<dyn Error>> {
        let response = self
            .send_command(format!(
                "set MIXER:Current/InCh/Fader/On {channel} 0 {}\n",
                if muted { 1 } else { 0 }
            ))
            .await?;
        println!("{response}");
        Ok(())
    }

    async fn fade(
        &mut self,
        channel: u16,
        mut initial_value: i32,
        mut final_value: i32,
        duration_ms: u64,
    ) -> Result<(), Box<dyn Error>> {
        initial_value = initial_value.clamp(self.min_fader_val, self.max_fader_val);
        final_value = final_value.clamp(self.min_fader_val, self.max_fader_val);

        let num_steps: u64 = duration_ms / 50;
        let step_delta: i32 = (final_value - initial_value) / (num_steps as i32);

        let mut interval = time::interval(time::Duration::from_millis(50));
        let mut current_value = initial_value;

        for _i in 0..num_steps {
            interval.tick().await;

            self.set_fader_level(channel, current_value).await?;
            println!("Set channel {channel} to {current_value}");

            current_value += step_delta;
        }

        final_value = if final_value == self.min_fader_val { self.neg_inf_val } else { final_value };

        self.set_fader_level(channel, final_value).await?;
        println!("Set channel {channel} to {final_value}");

        Ok(())
    }
}
