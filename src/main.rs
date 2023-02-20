use futures::future;
use std::error::Error;
use std::str::FromStr;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time;

#[tokio::main]
async fn main() {
    let mut mixer = Mixer::new("192.168.0.128:49280").await.unwrap();
    let mut mixer2 = Mixer::new("192.168.0.128:49280").await.unwrap();
    println!("Connected to mixer!");

    println!("{:?}", mixer.color(0).await.unwrap());
    mixer.set_color(1, LabelColor::Blue).await.unwrap();

    mixer.fader_level(1).await.unwrap();
    // mixer.fade(1, 10_00, -138_00, 10_000).await.unwrap();
    // time::sleep(time::Duration::from_secs(1)).await;
    // mixer.fade(1, -138_00, 10_00, 1_000).await.unwrap();

    let chan1_fader = mixer.fade(1, 10_00, -40_00, 3_000);
    let chan2_fader = mixer2.fade(2, -40_00, 10_00, 3_000);
    let results = future::join(chan1_fader, chan2_fader).await;
    results.0.unwrap();
    results.1.unwrap();
    mixer.set_fader_level(1, -138_00).await.unwrap();

    time::sleep(time::Duration::from_secs(3)).await;

    let chan1_fader = mixer.fade(1, -40_00, 10_00, 3_000);
    let chan2_fader = mixer2.fade(2, 10_00, -40_00, 3_000);
    let results = future::join(chan1_fader, chan2_fader).await;
    results.0.unwrap();
    results.1.unwrap();
    mixer2.set_fader_level(2, -138_00).await.unwrap();

    mixer.muted(0).await.unwrap();
    mixer.set_muted(0, true).await.unwrap();
}

#[derive(Debug)]
enum LabelColor {
    Purple,
    Pink,
    Red,
    Orange,
    Yellow,
    Blue,
    SkyBlue,
    Green,
}

impl LabelColor {
    pub fn to_string(&self) -> String {
        match self {
            Self::Purple => "Purple",
            Self::Pink => "Pink",
            Self::Red => "Red",
            Self::Orange => "Orange",
            Self::Yellow => "Yellow",
            Self::Blue => "Blue",
            Self::SkyBlue => "SkyBlue",
            Self::Green => "Green",
        }
        .to_string()
    }
}

impl FromStr for LabelColor {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Purple" => Ok(Self::Purple),
            "Pink" => Ok(Self::Pink),
            "Red" => Ok(Self::Red),
            "Orange" => Ok(Self::Orange),
            "Yellow" => Ok(Self::Yellow),
            "Blue" => Ok(Self::Blue),
            "SkyBlue" => Ok(Self::SkyBlue),
            "Green" => Ok(Self::Green),
            _ => Err(()),
        }
    }
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

    async fn color(&mut self, channel: u16) -> Result<LabelColor, Box<dyn Error>> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Label/Color {channel} 0\n"))
            .await?;
        let response = response.trim();

        if response.starts_with("ERROR") {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                response,
            )));
        }

        let split = response.split("\n");
        let mut response_val = "";
        for item in split {
            if !item.starts_with("OK") {
                continue;
            }

            response_val = item.split(" ").last().unwrap();

            break;
        }

        Ok(LabelColor::from_str(&(response_val.replace("\"", ""))).unwrap())
    }

    async fn set_color(&mut self, channel: u16, color: LabelColor) -> Result<(), Box<dyn Error>> {
        let response = self
            .send_command(format!(
                "set MIXER:Current/InCh/Label/Color {channel} 0 \"{}\"\n",
                color.to_string()
            ))
            .await?;
        let response = response.trim();

        if response.starts_with("ERROR") {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                response,
            )));
        }

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

        final_value = if final_value == self.min_fader_val {
            self.neg_inf_val
        } else {
            final_value
        };

        self.set_fader_level(channel, final_value).await?;
        println!("Set channel {channel} to {final_value}");

        Ok(())
    }
}
