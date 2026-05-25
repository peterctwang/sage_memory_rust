//! Live smoke test against the local `claude` binary.
//! Run with:  cargo run --example smoke_claude_cli --features claude-cli -p sage-llm
//!
//! If `claude` is not installed or auth-less, prints the underlying error and
//! exits 0 (smoke test, not a unit gate).

use sage_llm::{ChatMessage, ChatRequest, ClaudeCliLlm, LlmClient};

#[tokio::main]
async fn main() {
    let llm = match std::env::var("SAGE_CLAUDE_BIN") {
        Ok(p) => ClaudeCliLlm::new().with_binary(p),
        Err(_) => ClaudeCliLlm::new(),
    };
    eprintln!("[smoke] binary = {}, model = {}", llm.binary(), llm.model());
    let req = ChatRequest {
        messages: vec![
            ChatMessage::system(
                "You are a knowledge-graph triple extractor. \
                 Respond with ONLY a JSON object: \
                 {\"triples\":[{\"src\":..,\"rel\":..,\"dst\":..}],\"stop\":bool}",
            ),
            ChatMessage::user("Extract triples from: 'Alice founded Acme Industries in 2023.'"),
        ],
        temperature: 0.0,
        max_tokens: Some(256),
    };
    match llm.complete(req).await {
        Ok(resp) => {
            println!("--- claude response ---");
            println!("{}", resp.content);
        }
        Err(e) => eprintln!("[smoke] live call failed (expected if no claude on PATH): {e}"),
    }
}
