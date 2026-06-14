use crate::gateway::GatewayPacket;
use reqwest::Client;
use serde_json::json;
use tokio::time::{self, Duration};

mod commands;


/// Returns `true` if a reconnect is needed (e.g. token expired).
async fn handle_packet_c2c_message_create(packet_inbound: GatewayPacket, token: &String, node_name: &str) -> bool {
    let msg_data = &packet_inbound.d;

    let content = msg_data.get("content").and_then(|v| v.as_str()).unwrap_or("").trim();
    let msg_id = msg_data.get("id").and_then(|v| v.as_str()).unwrap_or("");

    let user_openid = msg_data
        .get("author")
        .and_then(|author| author.get("user_openid"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    println!("[INFO in dispatch] get signal: '{}' | from OpenID: {}", content, user_openid);

    match commands::match_command(content, node_name) {
        Some(reply) => {
            let is_err = send_c2c_reply(token, user_openid, msg_id, &reply).await.is_err();
            is_err
        }
        None => false,
    }
}



/// Returns `true` if a reconnect is needed (e.g. token expired).
pub async fn handle_packet(packet_inbound: GatewayPacket, token: &String, node_name: &str) -> bool {
    println!("[INFO in dispatch] Get inbound packet \"{:?}\" from gateway.", &packet_inbound.t);
    if packet_inbound.t.as_deref() == Some("C2C_MESSAGE_CREATE") {
        handle_packet_c2c_message_create(packet_inbound, token, node_name).await
    } else {
        false
    }
}


async fn send_c2c_reply(token: &str, user_openid: &str, msg_id: &str, content: &str) -> Result<(), ()> {
    let client = Client::new();
    
    let url = format!("https://api.sgroup.qq.com/v2/users/{}/messages", user_openid);

    let payload = json!({
        "content": content,
        "msg_id": msg_id,
        "msg_type": 0,
    });

    // Retry up to 2 times on network errors, with 1s delay
    let max_retries = 2;
    for attempt in 0..=max_retries {
        let result = client.post(&url)
            .header("Authorization", format!("QQBot {}", token))
            .json(&payload)
            .send()
            .await;

        match result {
            Ok(resp) => {
                if resp.status().is_success() {
                    println!("[INFO in Dispatch] 🚀 HTTP reply sent successfully!");
                    return Ok(());
                } else {
                    println!("[WARN in Dispatch] HTTP status code abnormal: {:?}, triggering reconnect...", resp.status());
                    return Err(());
                }
            }
            Err(e) => {
                if attempt < max_retries {
                    println!("[WARN in Dispatch] Network error, retrying in 1s ({}/{}): {}", attempt + 1, max_retries, e);
                    time::sleep(Duration::from_secs(1)).await;
                } else {
                    println!("[ERROR in Dispatch] All retries exhausted, HTTP network error: {}", e);
                    return Err(());
                }
            }
        }
    }
    Err(())
}