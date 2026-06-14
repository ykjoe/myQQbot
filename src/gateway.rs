use serde::{Deserialize, Serialize};
use reqwest::Client;
use serde_json::Value;
use tokio::{net::TcpStream, time::{self, Duration}};
use tokio_tungstenite::{MaybeTlsStream, connect_async, WebSocketStream, tungstenite::{Message}};
use futures_util::{stream::StreamExt, sink::SinkExt};

/////////////////////////////////////////////////////////
/// struct to get data
/////////////////////////////////////////////////////////
#[derive(Debug, Deserialize, Clone)]
pub struct QQBotInfo {
    pub appid: String,
    pub appsecret: String,
    pub node_name: String,
}

/////////////////////////////////////////////////////////
/// struct to post data
/////////////////////////////////////////////////////////
#[derive(Debug, Serialize)]
pub struct IdentifyPacket {
    pub op: u8,
    pub d: IdentifyData,
}

#[derive(Debug, Serialize)]
pub struct IdentifyData {
    pub token: String,
    pub intents: u32,
    pub shard: [u32; 2],
    pub properties: SystemProperties,
}

#[derive(Debug, Serialize)]
pub struct SystemProperties {
    #[serde(rename = "$os")]
    pub os: String,
    #[serde(rename = "$browser")]
    pub browser: String,
    #[serde(rename = "$device")]
    pub device: String,
}

/////////////////////////////////////////////////////////
/// struct here
/////////////////////////////////////////////////////////
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: String,
}

#[derive(Debug, Deserialize)]
struct HelloPayload {
    op: u8,
    d: HeartbeatInfo,
}

#[derive(Debug, Deserialize)]
struct HeartbeatInfo {
    heartbeat_interval: u64,
}

#[derive(Debug, Deserialize)]
pub struct GatewayPacket {
    pub op: u8,
    pub s: Option<u64>,
    pub t: Option<String>,
    pub d: serde_json::Value,
}

/////////////////////////////////////////////////////////
/// Functions
/////////////////////////////////////////////////////////

/////////////////////////////////////////////////////////////////////////////////////////////
/// Executes the bot's standard power-on sequence and anchors the inbound network pipeline.
///
/// This function acts as the primary hardware-like bootstrap loader for the gateway module.
/// It operates sequentially to establish the upstream communication interface:
///
/// 1. **Credential Exchange**: Asserts client identity against the Tencent Auth Gateway 
///    to fetch a temporary HTTP Bearer Token.
/// 2. **Bus Initialization**: Forwards the token to `establish_ws` to hook up the physical 
///    wire, parse the 'Hello' frame, and complete the 'Identify' protocol handshake.
/// 3. **Data Pump Ingestion**: Pass the fully active, authenticated `ws_stream` bus line 
///    into `read_ws` to power up the infinite event ingestion loop.
///
/// # Arguments
///
/// * `config` - A `QQBotInfo` configuration block containing the target node specification
///   and hardware/app credentials.
///
/// # Returns
///
/// * `Ok((ws_stream, token, heartbeat_interval))` on success.
/// * `Err(())` if any stage of the pipeline (Token, Handshake, or Main Loop) collapses.
/////////////////////////////////////////////////////////////////////////////////////////////
pub async fn connect_gateway(config: QQBotInfo) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, String, u64), ()> {
    let token: String = get_access_token(&config.appid, &config.appsecret, &config.node_name)
        .await
        .ok_or(())?;

    let (ws_stream, heartbeat_interval) = establish_ws(&token)
        .await?;

    Ok((ws_stream, token, heartbeat_interval))
}



pub async fn server_start <F, Fut> (
    mut ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    mut dispatch_fn: F,
    heartbeat_interval: u64,
) -> Result<(), ()>
where
    F: FnMut(GatewayPacket) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    println!("[INFO in Gateway] Local server start!! Heartbeat interval: {} ms", heartbeat_interval);

    let mut last_s: Option<u64> = None;
    let mut interval = time::interval(Duration::from_millis(heartbeat_interval));
    interval.tick().await; // 跳过首次立即触发

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let payload = serde_json::json!({"op": 1, "d": last_s});
                if let Ok(text) = serde_json::to_string(&payload) {
                    let _ = ws_stream.send(Message::Text(text.into())).await;
                }
            }
            result = read_ws(&mut ws_stream) => {
                match result {
                    Ok(Some(inbound_tx)) => {
                        if let Some(s) = inbound_tx.s {
                            last_s = Some(s);
                        }
                        if dispatch_fn(inbound_tx).await {
                            println!("[WARN in Gateway] Dispatch requested reconnect, tearing down...");
                            return Err(());
                        }
                    }
                    Ok(None) => continue,
                    Err(_) => return Err(()),
                }
            }
        }
    }
}


/// Requests or refreshes the access token from the Tencent OpenAPI server.
///
/// This is an active client-side operation that exchanges the application's credentials
/// (`appid` and `appsecret`) via a `POST` request for a temporary bearer token,
/// which is required for the subsequent WebSocket handshake.
///
/// # Arguments
///
/// * `appid` - The unique application ID provided by the Tencent Bot developer console.
/// * `appsecret` - The secure client secret used to authenticate the bot application.
/// * `node_name` - The identifier of the current VPS deployment node, utilized for cluster logging.
///
/// # Returns
///
/// * `Option<String>` - Returns `Some(access_token)` if the exchange succeeds. 
///   Returns `None` if a network timeout, DNS resolution failure, or an invalid JSON payload 
///   occurs (errors will be intercepted and logged internally).
async fn get_access_token(appid: &str, appsecret: &str, node_name: &str) -> Option<String> {
    let auth_url: &str = "https://bots.qq.com/app/getAppAccessToken";
    let client: Client = Client::new();

    println!("[INFO in Gateway] Node [{}] sending \"POST\" request to Tencent auth gateway...", node_name);

    let payload: Value = serde_json::json!({
        "appId": appid,
        "clientSecret": appsecret
    });

    match client.post(auth_url).json(&payload).send().await {
        Ok(response) => {
            match response.json::<TokenResponse>().await {
                Ok(token_data) => {
                    println!("[SUCCESS in Gateway] Token exchanged. Expires in: {} seconds", token_data.expires_in);
                    Some(token_data.access_token)
                }
                Err(e) => {
                    println!("[ERROR in Gateway] HTTP 200 OK received, but failed to parse Token JSON payload: {}", e);
                    None
                }
            }
        }
        Err(e) => {
            println!("[ERROR in Gateway] Network error. Target gateway might be timeout or unreachable: {}", e);
            None
        }
    }
}



/// Establishes the physical WebSocket connection and completes the gateway handshake.
///
/// This layer initializes the transport link, parses the remote 'Hello' config frame,
/// and transmits the 'Identify' authorization payload. Once the handshake is verified,
/// it hands over the fully active stream to the caller.
///
/// # Returns
///
/// * `Ok((ws_stream, heartbeat_interval))` — the active WebSocket stream and the
///   heartbeat interval (in milliseconds) required by the server.
pub async fn establish_ws(token: &String) -> Result<(WebSocketStream<MaybeTlsStream<TcpStream>>, u64),()> {
    let ws_url: &str = "wss://api.sgroup.qq.com/websocket";
    let (mut ws_stream, _) = match connect_async(ws_url).await{
        Ok(stream) => {
            println!("[INFO in Gateway] WebSocket connection established successfully!");
            stream
        }
        Err(e) => {
            println!("[ERROR in Gateway] WebSocket connection failed: {}", e);
            return Err(());
        }
    };
    
    // 解析 Hello 包 (op=0)，提取 heartbeat_interval
    let hello_text = match ws_stream.next().await {
        Some(Ok(Message::Text(text))) => text,
        Some(Ok(_)) => {
            println!("[ERROR in Gateway] Received unexpected frame type instead of text Hello.");
            return Err(());
        }
        Some(Err(e)) => {
            println!("[ERROR in Gateway] Failed during reading Hello message: {}", e);
            return Err(());
        }
        None => {
            println!("[ERROR in Gateway] Connection closed by remote before Hello message.");
            return Err(());
        }
    };

    let heartbeat_interval = match serde_json::from_str::<HelloPayload>(&hello_text) {
        Ok(hello_data) => {
            println!("[INFO in Gateway] Hello packet parsed. Heartbeat interval: {} ms", hello_data.d.heartbeat_interval);
            hello_data.d.heartbeat_interval
        }
        Err(e) => {
            println!("[ERROR in Gateway] Failed to parse Hello payload JSON: {}", e);
            return Err(());
        }
    };

    let identify_payload = IdentifyPacket {
        op: 2,
        d: IdentifyData {
            token: format!("QQBot {}", token),
            intents: 1 << 25,
            shard: [0, 1],
            properties: SystemProperties {
                os: "linux".to_string(),
                browser: "VpsWatchdog".to_string(),
                device: "OracleCloud".to_string(),
            },
        },
    };

    let identify_text = match serde_json::to_string(&identify_payload) {
        Ok(json_str) => json_str,
        Err(e) => {
            println!("[ERROR in Gateway] Failed to serialize IdentifyPacket: {}", e);
            return Err(());
        }
    };

    match ws_stream.send(Message::Text(identify_text.into())).await {
        Ok(_) => {
            println!("[INFO in Gateway] 🚀 [单聊/群聊总线] 握手成功！开始静默监听私聊信号...");
        }
        Err(e) => {
            println!("[ERROR in Gateway] Failed to send identify handshake to gateway: {}", e);
            return Err(());
        }
    };

    Ok((ws_stream, heartbeat_interval))
}



/// Hosts the infinite message pump loop over an established WebSocket stream.
///
/// This worker ingests inbound frames directly from the transport layer, decodes
/// them into gateway packets, and prepares them for the central event dispatcher.
///
/// # Returns
///
/// * `Ok(Some(packet))` — a valid gateway packet ready for dispatch.
/// * `Ok(None)` — a non-Text frame or unparseable payload; caller should continue.
/// * `Err(())` — the connection is dead; caller should tear down.
pub async fn read_ws(ws_stream: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,) -> Result<Option<GatewayPacket>, ()> {
    match ws_stream.next().await {
        Some(Ok(Message::Text(text))) => {
            match serde_json::from_str::<GatewayPacket>(&text) {
                Ok(inbound_tx) => {
                    println!("[DEBUG in Gateway] Inbound gateway packet cached. OP: {}, EVENT: {:?}", inbound_tx.op, inbound_tx.t);
                    Ok(Some(inbound_tx))
                    }
                Err(e) => {
                    println!("[WARN in Gateway] Dropping malformed or corrupted inbound JSON frame: {}", e);
                    Ok(None)
                }
            }
        }
        Some(Ok(Message::Close(frame))) => {
            println!("[WARN in Gateway] Remote server dispatched CLOSE frame. Tearing down connection line: {:?}", frame);
            Err(())
        }
        Some(Err(e)) => {
            println!("[ERROR in Gateway] Physical socket transport exception captured: {}", e);
            Err(())
        }
        None => {
            println!("[ERROR in Gateway] Stream channel exhausted. Remote link returned EOF (None).");
            Err(())
        }
        _ => {
            // Ping, Pong, Binary — 忽略，继续运行
            Ok(None)
        }
    }
}
