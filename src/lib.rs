use std::error::Error;
use std::str::FromStr;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time;

#[derive(Debug)]
pub enum LabelColor {
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
    type Err = String;

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
            _ => Err(format!("unknown LabelColor descriptor: {s}")),
        }
    }
}

pub struct Mixer {
    stream: TcpStream,
    max_fader_val: i32,
    min_fader_val: i32,
    neg_inf_val: i32,
    debug: bool,
}

impl Mixer {
    pub async fn new(addr: &str) -> Result<Self, Box<dyn Error>> {
        Ok(Mixer {
            stream: TcpStream::connect(addr).await?,
            max_fader_val: 10_00,
            min_fader_val: -138_00,
            neg_inf_val: -327_68,
            debug: false,
        })
    }

    pub fn set_debug(&mut self, d: bool) {
        self.debug = d;
    }

    async fn send_command(&mut self, cmd: String) -> Result<String, Box<dyn Error>> {
        if self.debug {
            println!("Sending command: {cmd}");
        }

        self.stream.write_all(cmd.as_bytes()).await?;

        tokio::time::sleep(time::Duration::from_millis(5)).await;

        let mut all_bytes = Vec::new();
        let buffer_size = 4096;

        loop {
            let mut buffer = vec![0; buffer_size];
            match self.stream.read(&mut buffer).await {
                Ok(n) => {
                    if n == 0 {
                        break;
                    } else {
                        for ele in buffer {
                            all_bytes.push(ele);
                        }

                        if n < buffer_size {
                            break;
                        }
                    }
                }
                Err(e) => return Err(Box::new(e)),
            }
        }

        if self.debug {
            println!("Bytes: {all_bytes:?}");
        }
        let result_str = std::str::from_utf8(&all_bytes)?;
        if self.debug {
            println!("NullStr: {result_str}");
        }
        let result_str = result_str.replace("\0", "");
        if self.debug {
            println!("Final: {result_str}");
        }

        for line in result_str.split("\n") {
            if self.debug {
                println!("Evaluating: {line}");
            }
            
            if line.starts_with("ERROR") {
                return Err(Box::new(RCPError {
                    message: line.to_owned(),
                }));
            } else if line.starts_with("OK") {
                return Ok(line.to_owned());
            }
        }

        Err(Box::new(RCPError {
            message: "Could not find response line from mixer".to_owned(),
        }))
    }

    pub async fn fader_level(&mut self, channel: u16) -> Result<i32, Box<dyn Error>> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Fader/Level {channel} 0\n"))
            .await?;
        let response = response.replace("\0", "");

        if response.starts_with("ERROR") {
            return Err(Box::new(RCPError { message: response }));
        }

        let split = response.split("\n");
        let mut response_val = 0;
        for item in split {
            if !item.starts_with("OK") {
                continue;
            }

            let opt = item.split(" ").last();
            if opt.is_none() {
                return Err(Box::new(RCPError {
                    message: "Couldn't find the last item".to_owned(),
                }));
            }

            // The following unwrap call should be safe because of the above if statement checking
            // the Option's value
            response_val = opt.unwrap().parse()?;

            break;
        }

        Ok(response_val)
    }

    pub async fn set_fader_level(
        &mut self,
        channel: u16,
        value: i32,
    ) -> Result<(), Box<dyn Error>> {
        let response = self
            .send_command(format!(
                "set MIXER:Current/InCh/Fader/Level {channel} 0 {value}\n"
            ))
            .await?;
        let response = response.replace("\0", "");

        if response.starts_with("ERROR") {
            return Err(Box::new(RCPError { message: response }));
        }

        // Technically, this RCP call returns the actually set value, which we could capture and
        // return to the consumer.
        Ok(())
    }

    pub async fn muted(&mut self, channel: u16) -> Result<bool, Box<dyn Error>> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Fader/On {channel} 0\n"))
            .await?;
        let response = response.replace("\0", "");

        if response.starts_with("ERROR") {
            return Err(Box::new(RCPError { message: response }));
        }

        let split = response.split("\n");
        let mut response_val = false;
        for item in split {
            if !item.starts_with("OK") {
                continue;
            }

            let opt = item.split(" ").last();
            if opt.is_none() {
                return Err(Box::new(RCPError {
                    message: "Could not get last item in list".to_owned(),
                }));
            }

            // The following unwrap call should be safe because of the above if statement checking
            // the Option's value
            let opt_val = opt.unwrap();

            response_val = if opt_val == "0" { false } else { true };

            break;
        }

        Ok(response_val)
    }

    pub async fn set_muted(&mut self, channel: u16, muted: bool) -> Result<(), Box<dyn Error>> {
        let response = self
            .send_command(format!(
                "set MIXER:Current/InCh/Fader/On {channel} 0 {}\n",
                if muted { 0 } else { 1 }
            ))
            .await?;
        let response = response.replace("\0", "");

        if response.starts_with("ERROR") {
            return Err(Box::new(RCPError { message: response }));
        }

        Ok(())
    }

    pub async fn color(&mut self, channel: u16) -> Result<LabelColor, Box<dyn Error>> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Label/Color {channel} 0\n"))
            .await?;
        let response = response.replace("\0", "");

        if response.starts_with("ERROR") {
            return Err(Box::new(RCPError { message: response }));
        }

        let split = response.split("\n");
        let mut response_val = "";
        for item in split {
            if !item.starts_with("OK") {
                continue;
            }

            let opt = item.split(" ").last();
            if opt.is_none() {
                return Err(Box::new(RCPError {
                    message: "could not get last item in list".to_string(),
                }));
            }

            // The following unwrap call should be safe because of the above if statement checking
            // the Option's value
            response_val = opt.unwrap();

            break;
        }

        Ok(LabelColor::from_str(&(response_val.replace("\"", ""))).unwrap())
    }

    pub async fn set_color(
        &mut self,
        channel: u16,
        color: LabelColor,
    ) -> Result<(), Box<dyn Error>> {
        let response = self
            .send_command(format!(
                "set MIXER:Current/InCh/Label/Color {channel} 0 \"{}\"\n",
                color.to_string()
            ))
            .await?;
        let response = response.replace("\0", "");

        if response.starts_with("ERROR") {
            return Err(Box::new(RCPError { message: response }));
        }

        Ok(())
    }

    pub async fn fade(
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
            if self.debug {
                println!("Set channel {channel} to {current_value}");
            }

            current_value += step_delta;
        }

        final_value = if final_value == self.min_fader_val {
            self.neg_inf_val
        } else {
            final_value
        };

        self.set_fader_level(channel, final_value).await?;
        if self.debug {
            println!("Set channel {channel} to {final_value}");
        }

        Ok(())
    }
}

#[derive(Debug)]
struct RCPError {
    message: String,
}

impl std::fmt::Display for RCPError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for RCPError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}
