//! 一次生成 HTML v2 主論點樹，或離線 replay 模型原始輸出。
//!
//! 線上：cargo run --example try_main_argument_tree -- transcript.json outline.json tree.json tree.html
//! 離線：cargo run --example try_main_argument_tree -- --replay transcript.json outline.json tree.json tree.html

use socrates_chat_backend::main_argument_tree::{
    MainTreeClient, PROMPT_VERSION, Transcript, parse_and_build, parse_transcript, render_html,
    sanitized_transcript,
};
use std::{
    env,
    error::Error,
    fs,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

fn save_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut f = OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(bytes)?;
    Ok(())
}
fn raw_path(path: &Path) -> PathBuf {
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("tree");
    path.with_file_name(format!("{name}.raw.json"))
}
fn read_input(path: &Path) -> Result<Transcript, Box<dyn Error>> {
    let input = parse_transcript(&fs::read_to_string(path)?)?;
    input.validate()?;
    Ok(input)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    let replay = args.get(1).is_some_and(|s| s == "--replay");
    let paths: Vec<&str> = if replay {
        args.iter().skip(2).map(String::as_str).collect()
    } else {
        args.iter().skip(1).map(String::as_str).collect()
    };
    if paths.len() != 4 {
        return Err("用法：try_main_argument_tree [--replay] transcript.json outline.json tree.json tree.html".into());
    }
    let input = read_input(Path::new(paths[0]))?;
    // 模型看到的是遮蔽後的內容；離線重驗必須使用同一份遮蔽內容比對引文。
    let sanitized = sanitized_transcript(&input)?;
    let raw: String;
    if replay {
        raw = fs::read_to_string(paths[1])?;
    } else {
        let client = MainTreeClient::from_env()?;
        let completion = client.complete(&input).await?;
        eprintln!(
            "API usage: {}",
            completion.usage.unwrap_or(serde_json::Value::Null)
        );
        eprintln!(
            "API request_id: {}",
            completion.request_id.as_deref().unwrap_or("unknown")
        );
        eprintln!(
            "API model: {}; prompt: {}",
            completion.model, PROMPT_VERSION
        );
        raw = completion.content;
        // 原始 JSON 先保存，讓驗證失敗時可定位問題，不會自動補邊／修 ID。
        let saved_raw = raw_path(Path::new(paths[1]));
        save_new(&saved_raw, raw.as_bytes())?;
        eprintln!("原始模型 JSON：{}", saved_raw.display());
        if sanitized
            .rounds
            .iter()
            .zip(&input.rounds)
            .any(|(a, b)| a.user_answer != b.user_answer || a.ai_question != b.ai_question)
        {
            let src_path = Path::new(paths[1]).with_extension("sanitized-input.json");
            save_new(
                &src_path,
                serde_json::to_string_pretty(&sanitized)?.as_bytes(),
            )?;
            eprintln!("遮蔽後輸入：{}", src_path.display());
        }
    }

    let (outline, tree) = parse_and_build(&sanitized, &raw)?;
    if !replay {
        save_new(
            Path::new(paths[1]),
            serde_json::to_string_pretty(&outline)?.as_bytes(),
        )?;
    }
    save_new(
        Path::new(paths[2]),
        serde_json::to_string_pretty(&tree)?.as_bytes(),
    )?;
    save_new(Path::new(paths[3]), render_html(&tree).as_bytes())?;
    println!(
        "PASS: {} nodes; root={}; tree={} html={}",
        tree.node_count, tree.root.node.id, paths[2], paths[3]
    );
    println!("僅驗證結構與原文；支持／反駁的語意仍須人工複核");
    Ok(())
}
