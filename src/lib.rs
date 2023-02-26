use std::error::Error;
use std::str::FromStr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{tcp::OwnedWriteHalf, TcpStream};
use tokio::sync::{mpsc, mpsc::Receiver};
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
    stream_writer: OwnedWriteHalf,
    recv_channel: Receiver<String>,
    max_fader_val: i32,
    min_fader_val: i32,
    neg_inf_val: i32,
    debug: bool,
}

impl Mixer {
    pub async fn new(addr: &str) -> Result<Self, Box<dyn Error>> {
        let (tx, rx) = mpsc::channel::<String>(16);
        let stream = TcpStream::connect(addr).await?;
        let (mut reader, writer) = stream.into_split();

        tokio::spawn(async move {
            let buffer_size = 512;

            loop {
                let mut line = Vec::new();
                let mut buffer = vec![0; buffer_size];
                match reader.read(&mut buffer).await {
                    Ok(_) => {
                        for ele in buffer {
                            match ele {
                                0xA => {
                                    let result = std::str::from_utf8(&line).unwrap();
                                    println!("Received: {result}");

                                    if result.starts_with("ERROR") || result.starts_with("OK") {
                                        println!("Sending: {result}");
                                        tx.send(result.to_owned()).await.unwrap();
                                    } else {
                                        println!("Dropping: {result}");
                                    }

                                    line.clear();
                                }
                                _ => line.push(ele),
                            }
                        }
                    }
                    Err(e) => return Err::<(), Box<std::io::Error>>(Box::new(e)),
                }
            }
        });

        Ok(Mixer {
            stream_writer: writer,
            recv_channel: rx,
            max_fader_val: 10_00,
            min_fader_val: -138_00,
            neg_inf_val: -327_68,
            debug: false,
        })
    }

    pub fn set_debug(&mut self, d: bool) {
        self.debug = d;
    }

    async fn send_command(&mut self, mut cmd: String) -> Result<String, Box<dyn Error>> {
        cmd.push('\n');

        if self.debug {
            println!("Sending command: {cmd}");
        }

        self.stream_writer.write_all(cmd.as_bytes()).await?;

        match self.recv_channel.recv().await {
            Some(v) => {
                if v.starts_with("ERROR") {
                    return Err(Box::new(RCPError {
                        message: v.to_owned(),
                    }));
                } else if v.starts_with("OK") {
                    return Ok(v);
                } else {
                    return Err(Box::new(RCPError {
                        message: format!("received message did not start with ERROR or OK: {v}"),
                    }));
                }
            }
            None => {
                return Err(Box::new(RCPError {
                    message: "closed channel from reader task".to_owned(),
                }));
            }
        }
    }

    pub async fn fader_level(&mut self, channel: u16) -> Result<i32, Box<dyn Error>> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Fader/Level {channel} 0"))
            .await?;

        match response.split(" ").last() {
            Some(v) => Ok(v.parse::<i32>()?),
            None => Err(Box::new(RCPError {
                message: "Couldn't find the last item".to_owned(),
            })),
        }
    }

    pub async fn set_fader_level(
        &mut self,
        channel: u16,
        value: i32,
    ) -> Result<(), Box<dyn Error>> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Fader/Level {channel} 0 {value}"
        ))
        .await?;

        // Technically, this RCP call returns the actually set value, which we could capture and
        // return to the consumer.
        Ok(())
    }

    pub async fn muted(&mut self, channel: u16) -> Result<bool, Box<dyn Error>> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Fader/On {channel} 0"))
            .await?;

        match response.split(" ").last() {
            Some(v) => Ok(if v == "0" { false } else { true }),
            None => Err(Box::new(RCPError {
                message: "Could not get last item in list".to_owned(),
            })),
        }
    }

    pub async fn set_muted(&mut self, channel: u16, muted: bool) -> Result<(), Box<dyn Error>> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Fader/On {channel} 0 {}",
            if muted { 0 } else { 1 }
        ))
        .await?;

        Ok(())
    }

    pub async fn color(&mut self, channel: u16) -> Result<LabelColor, Box<dyn Error>> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Label/Color {channel} 0"))
            .await?;

        match response.split(" ").last() {
            Some(v) => Ok(LabelColor::from_str(&(v.replace("\"", "")))?),
            None => Err(Box::new(RCPError {
                message: "could not get last item in list".to_string(),
            })),
        }
    }

    pub async fn set_color(
        &mut self,
        channel: u16,
        color: LabelColor,
    ) -> Result<(), Box<dyn Error>> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Label/Color {channel} 0 \"{}\"",
            color.to_string()
        ))
        .await?;

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
