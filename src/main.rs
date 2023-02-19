use std::io::prelude::*;
use std::net::TcpStream;

fn main() -> std::io::Result<()> {
    let mut stream = TcpStream::connect("192.168.0.128:49280")?;
    println!("stream connected");

    for fader_val in (-32768..0).rev() {
        stream.write(format!("set MIXER:Current/InCh/Fader/Level 1 0 {}\n", fader_val).as_bytes())?;
        println!("stream written to");
    }

    // let mut response = [0; 128];
    // stream.read(&mut response)?;
    //
    // println!("{} {:?}", std::str::from_utf8(&response).unwrap(), response);

    Ok(())
}
