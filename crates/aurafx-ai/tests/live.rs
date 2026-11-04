//! Live provider test. Run with: AURAFX_LIVE_MODEL=<model> cargo test -p aurafx-ai -- --ignored
use aurafx_ai::*;

#[test]
#[ignore]
fn ollama_stream() {
    let mut cfg = AiConfig::default();
    cfg.model = std::env::var("AURAFX_LIVE_MODEL").unwrap_or_else(|_| "glm-5.3-flash:cloud".into());
    let models = list_models(&cfg).expect("list models");
    assert!(!models.is_empty(), "no models at {}", cfg.ollama_url);
    let sys = system_prompt("- [1] \"wavelet.vti\" (VTI), visible, ACTIVE\n    Image Data: 9261 points\n    point arrays:\n      RTData (Float32 x 1, range 37 .. 277)", "- slice: Cut the data with a plane [type, origin, normal]");
    let rx = stream_chat(cfg, sys, vec![ChatMessage { role: Role::User, content: "Slice the active dataset along z and color it by RTData. Reply with code only.".into() }]);
    let mut text = String::new();
    for ev in rx {
        match ev {
            StreamEvent::Delta(d) => text.push_str(&d),
            StreamEvent::Done => break,
            StreamEvent::Error(e) => panic!("{e}"),
        }
    }
    println!("REPLY:\n{text}");
    let blocks = python_blocks(&text);
    assert!(!blocks.is_empty(), "no python block in reply");
    assert!(blocks[0].contains("slice"));
}
