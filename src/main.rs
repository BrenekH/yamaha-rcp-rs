use anyhow::Result;
use obws::Client;
use time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    // Connect to the OBS instance through obs-websocket.
    let client = Client::connect("localhost", 4455, None::<String>).await?;

    // Get and print out version information of OBS and obs-websocket.
    let version = client.general().version().await?;
    println!("{:#?}", version);

    // Get a list of available scenes and print them out.
    let scene_list = client.scenes().list().await?;
    println!("{:#?}", scene_list);
    
    client.transitions().set_current_duration(Duration::new(1, 0)).await?;

    let transition_list = client.transitions().list().await?;
    println!("{transition_list:#?}");

    client.transitions().trigger().await?;

    Ok(())
}
