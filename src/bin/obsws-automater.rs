use anyhow::Result;
use obws::Client;
use time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::connect("localhost", 4455, None::<String>).await?;

    let list = client.scenes().list().await?;
    println!("{:#?}", list);

    client.scenes().set_current_program_scene("Looping Video").await?;

    Ok(())
}
