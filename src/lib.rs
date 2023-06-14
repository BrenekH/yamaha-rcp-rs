// We use an underscore to make our decibel values more readable, which
// Clippy by default does not agree with.
#![allow(clippy::inconsistent_digit_grouping)]

use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::net::SocketAddr;
use std::str::FromStr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{tcp::OwnedWriteHalf, TcpStream};
use tokio::sync::{mpsc, mpsc::Receiver};
use tokio::time;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("network error: {0}")]
    NetworkError(#[from] std::io::Error),
    #[error("invalid network address: {0}")]
    AddrParseError(#[from] std::net::AddrParseError),
    #[error("Yamaha Remote Control Protocol error: {0}")]
    RCPError(String),
    #[error("could not parse console response: {0}")]
    RCPParseError(#[from] Box<dyn std::error::Error>),
    #[error("{0}")]
    LabelColorParseError(String),
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
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
    type Err = Error;

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
            _ => Err(Error::LabelColorParseError(format!(
                "unknown LabelColor descriptor: {s}"
            ))),
        }
    }
}

#[derive(Debug)]
pub enum SceneList {
    A,
    B
}

impl Display for SceneList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::A => "scene_a",
                Self::B => "scene_b"
            }
        )
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
    pub async fn new(addr: &str) -> Result<Self, Error> {
        let (tx, rx) = mpsc::channel::<String>(16);

        let std_tcp_sock = std::net::TcpStream::connect_timeout(
            &addr.parse::<SocketAddr>()?,
            time::Duration::from_secs(3),
        )?;
        std_tcp_sock.set_nonblocking(true)?;

        let stream = TcpStream::from_std(std_tcp_sock)?;
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

    async fn send_command(&mut self, mut cmd: String) -> Result<String, Error> {
        cmd.push('\n');

        if self.debug {
            println!("Sending command: {cmd}");
        }

        self.stream_writer.write_all(cmd.as_bytes()).await?;

        match self.recv_channel.recv().await {
            Some(v) => {
                if v.starts_with("ERROR") {
                    Err(Error::RCPError(v))
                } else if v.starts_with("OK") {
                    return Ok(v);
                } else {
                    return Err(Error::RCPError(format!(
                        "received message did not start with ERROR or OK: {v}"
                    )));
                }
            }
            None => Err(Error::RCPError("closed channel from reader task".into())),
        }
    }

    async fn request_bool(&mut self, cmd: String) -> Result<bool, Error> {
        let response = self.send_command(cmd).await?;

        match response.split(' ').last() {
            Some(v) => Ok(v != "0"),
            None => Err(Error::RCPError("Could not get last item in list".into())),
        }
    }

    async fn request_int(&mut self, cmd: String) -> Result<i32, Error> {
        let response = self.send_command(cmd).await?;

        match response.split(' ').last() {
            Some(v) => Ok(v
                .parse::<i32>()
                .map_err(|e| Error::RCPParseError(Box::new(e)))?),
            None => Err(Error::RCPError("Couldn't find the last item".into())),
        }
    }

    async fn request_string(&mut self, cmd: String) -> Result<String, Error> {
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

    pub async fn fader_level(&mut self, channel: u16) -> Result<i32, Error> {
        self.request_int(format!("get MIXER:Current/InCh/Fader/Level {channel} 0"))
            .await
    }

    pub async fn set_fader_level(&mut self, channel: u16, value: i32) -> Result<(), Error> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Fader/Level {channel} 0 {value}"
        ))
        .await?;

        // Technically, this RCP call returns the actually set value, which we could capture and
        // return to the consumer.
        Ok(())
    }

    pub async fn muted(&mut self, channel: u16) -> Result<bool, Error> {
        self.request_bool(format!("get MIXER:Current/InCh/Fader/On {channel} 0"))
            .await
    }

    pub async fn set_muted(&mut self, channel: u16, muted: bool) -> Result<(), Error> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Fader/On {channel} 0 {}",
            if muted { 0 } else { 1 }
        ))
        .await?;

        Ok(())
    }

    pub async fn color(&mut self, channel: u16) -> Result<LabelColor, Error> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Label/Color {channel} 0"))
            .await?;

        match response.split(' ').last() {
            Some(v) => Ok(v.replace('\"', "").parse()?),
            None => Err(Error::RCPError("could not get last item in list".into())),
        }
    }

    pub async fn set_color(&mut self, channel: u16, color: LabelColor) -> Result<(), Error> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Label/Color {channel} 0 \"{}\"",
            color
        ))
        .await?;

        Ok(())
    }

    pub async fn label(&mut self, channel: u16) -> Result<String, Error> {
        self.request_string(format!("get MIXER:Current/InCh/Label/Name {channel} 0"))
            .await
    }

    pub async fn set_label(&mut self, channel: u16, label: &str) -> Result<(), Error> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Label/Name {channel} 0 \"{label}\""
        ))
        .await?;

        Ok(())
    }

    pub async fn recall_scene(&mut self, scene_list: SceneList, scene_number: u8) -> Result<(), Error> {
        self.send_command(format!("ssrecall_ex {scene_list} {scene_number}")).await?;
        Ok(())
    }

    pub async fn fade(
        &mut self,
        channel: u16,
        mut initial_value: i32,
        mut final_value: i32,
        duration_ms: u64,
    ) -> Result<(), Error> {
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
