use std::io::Read;
use std::time::Instant;

fn main() {
    let body = serde_json::json!({
        "model": "local",
        "messages": [{"role": "user", "content": "Tell me a short story."}],
        "stream": true,
        "temperature": 0.6,
        "top_p": 0.95,
        "top_k": 20,
    });

    let start = Instant::now();
    let response = ureq::post("http://127.0.0.1:8081/v1/chat/completions")
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .unwrap();

    let mut reader = response.into_reader();
    let mut buf = [0; 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let s = String::from_utf8_lossy(&buf[..n]);
                println!("{:.3}s: read {} bytes\n---{}---", start.elapsed().as_secs_f64(), n, s);
            }
            Err(e) => {
                println!("Error: {}", e);
                break;
            }
        }
    }
}
