use std::fs;

mod gateway;
mod dispatch;

#[tokio::main]
async fn main() -> Result<(), ()> {
    println!("[INFO] Read \"config.toml\".");

    let config_str: String = match fs::read_to_string("config.toml") {
        Ok(content) => content,
        Err(e) => {
            println!("[ERROR in main] Failed to read config.toml! syntax:{}", e);
            return Err(());
        }
        
    };

    let config: gateway::QQBotInfo = match toml::from_str(&config_str) {
        Ok(config) => config,
        Err(e) => {
            println!("[ERROR in main] Failed to parse config.toml! syntax{}", e);
            return Err(());
        }
    };

    let node_name = config.node_name.clone();

    loop {
        println!("[INFO] Connect to Tencent server.");
        let (ws_stream, token, heartbeat_interval) = match gateway::connect_gateway(config.clone()).await {
            Ok(result) => result,
            Err(_) => {
                println!("[ERROR in main] Failed to connect gateway, retrying in 3s...");
                tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                continue;
            }
        };

        println!("[INFO] Handshake success. Injecting credentials into dispatch pipeline...");
        let t = token.clone();
        let n = node_name.clone();
        if gateway::server_start(ws_stream, move |packet| {
            let t = t.clone();
            let n = n.clone();
            
            async move {
                dispatch::handle_packet(packet, &t, &n).await
            }
        }, heartbeat_interval).await.is_err() {
            println!("[WARN in main] Connection lost, reconnecting in 3s...");
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
            continue;
        }
    }
}
