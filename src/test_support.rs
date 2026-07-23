use crate::{config::Config, i18n::I18n, prompts::PromptStore};
use secrecy::SecretString;
use std::{collections::VecDeque, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex,
};

pub fn config(database_url: String, base_url: String, api_key: Option<&str>) -> Config {
    Config {
        database_url,
        local_user: "test".into(),
        glm_api_key: api_key.map(|value| SecretString::from(value.to_owned())),
        glm_base_url: base_url,
        glm_model: "test-model".into(),
        telegram_token: Some(SecretString::from("123:test".to_owned())),
        i18n: I18n::load("./resources", "en-US").unwrap(),
        prompts: PromptStore::load("./resources", "en-US").unwrap(),
        storage_enabled: false,
        storage_examples_path: "./resources/storage-examples.json".into(),
        storage_model_cache: ".fastembed_cache".into(),
        storage_min_similarity: 0.75,
        storage_min_margin: 0.03,
        storage_top_k: 3,
    }
}

pub async fn mock_server(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
    let handle = tokio::spawn(async move {
        let mut requests = vec![];
        loop {
            let response = {
                let mut responses = responses.lock().await;
                match responses.pop_front() {
                    Some(response) => response,
                    None => break,
                }
            };
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 16 * 1024];
            let size = socket.read(&mut bytes).await.unwrap();
            requests.push(String::from_utf8_lossy(&bytes[..size]).into_owned());
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        }
        requests
    });
    (format!("http://{address}"), handle)
}

pub fn glm_response(content: &str) -> String {
    serde_json::json!({"choices":[{"message":{"content":content}}]}).to_string()
}

pub fn telegram_response() -> String {
    serde_json::json!({
        "ok": true,
        "result": {
            "message_id": 2,
            "date": 0,
            "chat": {"id": 123, "type": "private"},
            "text": "ok"
        }
    })
    .to_string()
}
