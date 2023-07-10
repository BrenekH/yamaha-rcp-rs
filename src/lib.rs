#![allow(clippy::needless_doctest_main)]
/*!
# Yamaha Remote Control Protocol (Rust)

Remote control of [Yamaha mixing consoles](https://usa.yamaha.com/products/proaudio/mixers/index.html) using IP networking.

## Disclaimer

 > This library is mainly tested against the [Yamaha TF Series](https://usa.yamaha.com/products/proaudio/mixers/tf/index.html)
 > of consoles, specifically the TF1.
 > Fully tested compatibility of the
 > [Rivage PM](https://usa.yamaha.com/products/proaudio/mixers/rivage_pm/index.html),
 > [DM7](https://usa.yamaha.com/products/proaudio/mixers/dm7/index.html),
 > [DM3](https://usa.yamaha.com/products/proaudio/mixers/dm3/index.html),
 > [CL](https://usa.yamaha.com/products/proaudio/mixers/cl_series/index.html),
 > and [QL](https://usa.yamaha.com/products/proaudio/mixers/ql_series/index.html)
 > lines is the final goal of this library,
 > but I do not have access to any of these consoles to be able to test against.
 > If you do happen to have access and are willing to help out development, please [get in touch](https://github.com/BrenekH/yamaha-rcp-rs/discussions).

## Example

```no_run
use yamaha_rcp_rs::{Error, TFMixer};

#[tokio::main]
fn main() -> Result<(), Error> {
    let mixer = TFMixer::new("192.168.0.128:49280")?;

    // Set channel 1 to -10.00 dB
    mixer.set_fader_level(0, -10_00).await?;
}
```

## Extra Documentation

The following is a personal collection of documentation on Yamaha's mixer control protocol since
they don't provide any decent version of their own: [github.com/BrenekH/yamaha-rcp-docs](https://github.com/BrenekH/yamaha-rcp-docs#readme)
*/

// We use an underscore to make our decibel values more readable, which
// Clippy by default does not agree with.
#![allow(clippy::inconsistent_digit_grouping)]

use log::debug;
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{tcp::OwnedWriteHalf, TcpStream};
use tokio::sync::{mpsc, mpsc::Receiver, Mutex};
use tokio::time;

/// Enumeration of errors that originate from `yamaha_rcp_rs`
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
    #[error("{0}")]
    SceneListParseError(String),
}

/// All possible colors that the TF1 console can use for a channel
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
        match s.to_lowercase().as_str() {
            "purple" => Ok(Self::Purple),
            "pink" => Ok(Self::Pink),
            "red" => Ok(Self::Red),
            "orange" => Ok(Self::Orange),
            "yellow" => Ok(Self::Yellow),
            "blue" => Ok(Self::Blue),
            "skyblue" => Ok(Self::SkyBlue),
            "green" => Ok(Self::Green),
            _ => Err(Error::LabelColorParseError(format!(
                "unknown LabelColor descriptor: {s}"
            ))),
        }
    }
}

/// Possible scene lists that scenes may be stored in
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum SceneList {
    A,
    B,
}

impl Display for SceneList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::A => "scene_a",
                Self::B => "scene_b",
            }
        )
    }
}

impl FromStr for SceneList {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "a" => Ok(Self::A),
            "b" => Ok(Self::B),
            _ => Err(Error::SceneListParseError(format!(
                "unknown SceneList descriptor: {s}"
            ))),
        }
    }
}

/// Main client structure for TF series mixing consoles
///
/// Construct using [TFMixer::new]
#[derive(Clone, Debug)]
pub struct TFMixer {
    max_fader_val: i32,
    min_fader_val: i32,
    neg_inf_val: i32,
    socket_addr: SocketAddr,
    connections: Arc<Mutex<Vec<Connection>>>,
    num_connections: Arc<Mutex<u8>>,
    connection_limit: u8,
}

#[derive(Debug)]
struct Connection {
    writer: OwnedWriteHalf,
    recv_channel: Receiver<String>,
}

impl TFMixer {
    pub async fn new(addr: &str) -> Result<Self, Error> {
        let socket_addr: SocketAddr = addr.parse()?;

        let mixer = TFMixer {
            max_fader_val: 10_00,
            min_fader_val: -138_00,
            neg_inf_val: -327_68,
            socket_addr,
            connections: Arc::new(Mutex::new(vec![])),
            num_connections: Arc::new(Mutex::new(8)),
            connection_limit: 1,
        };

        let initial_connection = mixer.new_connection().await?;
        {
            let mut connections = mixer.connections.lock().await;
            let mut num_conns = mixer.num_connections.lock().await;
            connections.push(initial_connection);
            *num_conns += 1;
        }

        Ok(mixer)
    }

    pub fn set_connection_limit(&mut self, limit: u8) {
        self.connection_limit = limit;
    }

    async fn new_connection(&self) -> Result<Connection, Error> {
        let (tx, rx) = mpsc::channel::<String>(16);

        let std_tcp_sock =
            std::net::TcpStream::connect_timeout(&self.socket_addr, time::Duration::from_secs(3))?;
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

        Ok(Connection {
            writer,
            recv_channel: rx,
        })
    }

    async fn send_command(&self, mut cmd: String) -> Result<String, Error> {
        cmd.push('\n');

        debug!("Sending command: {cmd}");

        // Extract a connection from the connection pool while observing the connection limit
        let mut conn: Connection;
        {
            let mut conns = self.connections.lock().await;
            conn = match conns.pop() {
                Some(c) => c,
                None => {
                    let mut num_conns = self.num_connections.lock().await;
                    if *num_conns < self.connection_limit {
                        *num_conns += 1;
                        self.new_connection().await?
                    } else {
                        drop(num_conns);
                        let existing_conn: Connection;
                        loop {
                            drop(conns);
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            conns = self.connections.lock().await;
                            if let Some(c) = conns.pop() {
                                existing_conn = c;
                                break;
                            }
                        }

                        existing_conn
                    }
                }
            };
        }

        conn.writer.write_all(cmd.as_bytes()).await?;

        let result = match conn.recv_channel.recv().await {
            Some(v) => {
                if v.starts_with("ERROR") {
                    Err(Error::RCPError(v))
                } else if v.starts_with("OK") {
                    Ok(v)
                } else {
                    Err(Error::RCPError(format!(
                        "received message did not start with ERROR or OK: {v}"
                    )))
                }
            }
            None => Err(Error::RCPError("closed channel from reader task".into())),
        };

        // Add the connection we used back into the pool
        {
            let mut conns = self.connections.lock().await;
            conns.push(conn);
        }

        result
    }

    async fn request_bool(&self, cmd: String) -> Result<bool, Error> {
        let response = self.send_command(cmd).await?;

        match response.split(' ').last() {
            Some(v) => Ok(v != "0"),
            None => Err(Error::RCPError("Could not get last item in list".into())),
        }
    }

    async fn request_int(&self, cmd: String) -> Result<i32, Error> {
        let response = self.send_command(cmd).await?;

        match response.split(' ').last() {
            Some(v) => Ok(v
                .parse::<i32>()
                .map_err(|e| Error::RCPParseError(Box::new(e)))?),
            None => Err(Error::RCPError("Couldn't find the last item".into())),
        }
    }

    async fn request_string(&self, cmd: String) -> Result<String, Error> {
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

    pub async fn fader_level(&self, channel: u16) -> Result<i32, Error> {
        self.request_int(format!("get MIXER:Current/InCh/Fader/Level {channel} 0"))
            .await
    }

    pub async fn set_fader_level(&self, channel: u16, value: i32) -> Result<(), Error> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Fader/Level {channel} 0 {value}"
        ))
        .await?;

        // Technically, this RCP call returns the actually set value, which we could capture and
        // return to the consumer.
        Ok(())
    }

    pub async fn muted(&self, channel: u16) -> Result<bool, Error> {
        self.request_bool(format!("get MIXER:Current/InCh/Fader/On {channel} 0"))
            .await
    }

    pub async fn set_muted(&self, channel: u16, muted: bool) -> Result<(), Error> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Fader/On {channel} 0 {}",
            if muted { 0 } else { 1 }
        ))
        .await?;

        Ok(())
    }

    pub async fn color(&self, channel: u16) -> Result<LabelColor, Error> {
        let response = self
            .send_command(format!("get MIXER:Current/InCh/Label/Color {channel} 0"))
            .await?;

        match response.split(' ').last() {
            Some(v) => Ok(v.replace('\"', "").parse()?),
            None => Err(Error::RCPError("could not get last item in list".into())),
        }
    }

    pub async fn set_color(&self, channel: u16, color: LabelColor) -> Result<(), Error> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Label/Color {channel} 0 \"{}\"",
            color
        ))
        .await?;

        Ok(())
    }

    pub async fn label(&self, channel: u16) -> Result<String, Error> {
        self.request_string(format!("get MIXER:Current/InCh/Label/Name {channel} 0"))
            .await
    }

    pub async fn set_label(&self, channel: u16, label: &str) -> Result<(), Error> {
        self.send_command(format!(
            "set MIXER:Current/InCh/Label/Name {channel} 0 \"{label}\""
        ))
        .await?;

        Ok(())
    }

    pub async fn recall_scene(&self, scene_list: SceneList, scene_number: u8) -> Result<(), Error> {
        self.send_command(format!("ssrecall_ex {scene_list} {scene_number}"))
            .await?;
        Ok(())
    }

    pub async fn fade(
        &self,
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
            debug!("Set channel {channel} to {current_value}");

            current_value += step_delta;
        }

        final_value = if final_value == self.min_fader_val {
            self.neg_inf_val
        } else {
            final_value
        };

        self.set_fader_level(channel, final_value).await?;
        debug!("Set channel {channel} to {final_value}");

        Ok(())
    }
}
