// We use an underscore to make our decibel values more readable, which
// Clippy by default does not agree with.
#![allow(clippy::inconsistent_digit_grouping)]

use std::error::Error;
use std::fmt::Display;
use std::str::FromStr;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{tcp::OwnedWriteHalf, TcpStream};
use tokio::sync::{mpsc, mpsc::Receiver};
use tokio::time;

#[derive(Debug, Deserialize, Serialize)]
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

impl Display for LabelColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
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
        )
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

                                    if result.starts_with("ERROR") || result.starts_with("OK") {
                                        tx.send(result.to_owned()).await.unwrap();
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
                    Err(Box::new(RCPError { message: v }))
                } else if v.starts_with("OK") {
                    return Ok(v);
                } else {
                    return Err(Box::new(RCPError {
                        message: format!("received message did not start with ERROR or OK: {v}"),
                    }));
                }
            }
            None => Err(Box::new(RCPError {
                message: "closed channel from reader task".to_owned(),
            })),
        }
    }

    async fn request_bool(&mut self, cmd: String) -> Result<bool, Box<dyn Error>> {
        let response = self.send_command(cmd).await?;

        match response.split(' ').last() {
            Some(v) => Ok(v != "0"),
            None => Err(Box::new(RCPError {
                message: "Could not get last item in list".to_owned(),
            })),
        }
    }

    async fn request_int(&mut self, cmd: String) -> Result<i32, Box<dyn Error>> {
        let response = self.send_command(cmd).await?;

        match response.split(' ').last() {
            Some(v) => Ok(v.parse::<i32>()?),
            None => Err(Box::new(RCPError {
                message: "Couldn't find the last item".to_owned(),
            })),
        }
    }

    async fn request_string(&mut self, cmd: String) -> Result<String, Box<dyn Error>> {
        let response = self.send_command(cmd).await?;

        let mut resp_vec = Vec::new();
        let mut looking = false;
        for fragment in response.split(' ') {
            if !looking && fragment.starts_with('\"') && fragment.ends_with('\"') {
                resp_vec.push(fragment[1..fragment.len() - 1].to_owned());
                break;
            }

            if fragment.starts_with('\"') && !looking {
                looking = true;
                resp_vec.push(fragment[1..fragment.len()].to_owned());
                continue;
            }

            if fragment.ends_with('\"') && looking {
                resp_vec.push(fragment[0..fragment.len() - 1].to_owned());
                break;
            }

            if looking {
                resp_vec.push(fragment.to_owned());
            }
        }
        let label = resp_vec.join(" ");

        Ok(label)
    }

    pub async fn fader_level(&mut self, channel: u16) -> Result<i32, Box<dyn Error>> {
        self.request_int(format!("get MIXER:Current/InCh/Fader/Level {channel} 0"))
            .await
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
        self.request_bool(format!("get MIXER:Current/InCh/Fader/On {channel} 0"))
            .await
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

        match response.split(' ').last() {
            Some(v) => Ok(LabelColor::from_str(&(v.replace('\"', "")))?),
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
            color
        ))
        .await?;

        Ok(())
    }

    pub async fn label(&mut self, channel: u16) -> Result<String, Box<dyn Error>> {
        self.request_string(format!("get MIXER:Current/InCh/Label/Name {channel} 0"))
            .await
    }

    pub async fn set_label(&mut self, channel: u16, label: &str) -> Result<(), Box<dyn Error>> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Label/Name {channel} 0 \"{label}\""
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
