//! Local JSON -> database importer for testing the summary argument tree UI.
//!
//! Save as backend/examples/import_tree_chat.rs. From the repo root, load .env,
//! then cd backend and run:
//! cargo run --locked --example import_tree_chat -- /path/T01_chat.json
//! Optional second argument: an existing student ID (default: 11500001A).
//! Uses existing dependencies and DATABASE_URL; no Python, psql, or new crates.
//! Start the backend with DEMO_MODE=true once before importing.
//! Original messages are preserved. Identical normalized content for the same
//! student/language reuses the same Rust-import ID, preserving saved trees.
//! Ordinary learning summaries are not generated; their panel may show 404.

use std::{env, error::Error, fs, io, path::PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const DEFAULT_STUDENT: &str = "11500001A";
const CLASSROOM_ID: &str = "b81e26ed-2759-5a10-be20-05d32b52a483";

#[derive(Debug, Deserialize, Serialize)]
struct Chat {
    topic_id: String,
    title: String,
    #[serde(default)]
    description: String,
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Message {
    role: String,
    content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    question_type: Option<String>,
}

fn invalid(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidInput, message.into()).into()
}

fn db_error(error: sqlx::Error) -> Box<dyn Error> {
    let code = error
        .as_database_error()
        .and_then(|error| error.code())
        .map(|code| code.into_owned())
        .unwrap_or_else(|| "connection/runtime".into());
    // Do not echo PostgreSQL details containing private message text or URLs.
    invalid(format!(
        "資料庫操作失敗（{code}）；請確認 DATABASE_URL、migration 與示範帳號已準備完成"
    ))
}

fn parse(raw: &str) -> Result<Chat> {
    let mut chat: Chat = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
        .map_err(|_| invalid("JSON 格式不正確，需要 topic_id、title、messages"))?;
    for (field, value) in [
        ("topic_id", chat.topic_id.as_str()),
        ("title", chat.title.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(invalid(format!("{field} 不可為空")));
        }
    }
    if [&chat.topic_id, &chat.title, &chat.description]
        .iter()
        .any(|value| value.contains('\0'))
    {
        return Err(invalid("文字欄位不可含 NUL"));
    }
    for (index, message) in chat.messages.iter_mut().enumerate() {
        message.role = match message.role.as_str() {
            "ai" | "assistant" => "ai",
            "student" | "user" => "student",
            _ => return Err(invalid(format!("第 {} 則訊息 role 不受支援", index + 1))),
        }
        .into();
        if message.content.trim().is_empty()
            || message.content.contains('\0')
            || message
                .question_type
                .as_deref()
                .is_some_and(|value| value.contains('\0'))
        {
            return Err(invalid(format!("第 {} 則訊息不可為空或含 NUL", index + 1)));
        }
        if message.role == "student" {
            message.question_type = None;
        }
    }
    if !chat
        .messages
        .iter()
        .any(|message| message.role == "student")
    {
        return Err(invalid("至少需要一則學生回答"));
    }
    Ok(chat)
}

fn stable_id(tag: &str, content: &[u8]) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(tag.as_bytes());
    hasher.update([0]);
    hasher.update(content);
    let hash = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80; // UUID version 8 (custom SHA-256).
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

async fn import(pool: &PgPool, chat: &Chat, student_id: &str) -> Result<Uuid> {
    let mut tx = pool.begin().await.map_err(db_error)?;
    let student: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, language FROM users WHERE student_id = $1
         AND disabled_at IS NULL AND NOT is_admin",
    )
    .bind(student_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    let (student_uuid, language) =
        student.ok_or_else(|| invalid("找不到可用的學生帳號；先以 DEMO_MODE=true 啟動後端"))?;
    let teacher: Option<Uuid> = sqlx::query_scalar(
        "SELECT u.id FROM users u JOIN teachers t ON t.email = u.email
         WHERE u.email = 'demo.teacher@example.com'
           AND u.disabled_at IS NULL AND NOT u.is_admin",
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    let teacher = teacher.ok_or_else(|| invalid("找不到示範教師；先以 DEMO_MODE=true 啟動後端"))?;
    let classroom = Uuid::parse_str(CLASSROOM_ID)?;

    sqlx::query(
        "INSERT INTO classrooms (id, name, description, created_by)
         VALUES ($1, '論證樹本機匯入測試', '[local-import] JSON 對話測試', $2)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(classroom)
    .bind(teacher)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;
    let writable: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM classrooms
         WHERE id = $1 AND created_by = $2 AND NOT is_archived)",
    )
    .bind(classroom)
    .bind(teacher)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_error)?;
    if !writable {
        return Err(invalid("本機測試教室已封存或擁有者不同"));
    }
    sqlx::query(
        "INSERT INTO enrollments (classroom_id, user_id, student_id)
         VALUES ($1, $2, $3) ON CONFLICT (classroom_id, user_id) DO NOTHING",
    )
    .bind(classroom)
    .bind(student_uuid)
    .bind(student_id)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;

    // Source IDs such as T01 are not database UUIDs; use a stable mapping.
    let topic = stable_id(
        "socrates-chat/local-rust-import/topic/v1",
        &serde_json::to_vec(&(&chat.topic_id, &chat.title))?,
    );
    sqlx::query(
        "INSERT INTO topics (id, title, description) VALUES ($1, $2, $3)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(topic)
    .bind(&chat.title)
    .bind(&chat.description)
    .execute(&mut *tx)
    .await
    .map_err(db_error)?;
    let id = stable_id(
        "socrates-chat/local-rust-import/conversation/v1",
        &serde_json::to_vec(&(student_uuid, &language, chat))?,
    );
    let count = i32::try_from(
        chat.messages
            .iter()
            .filter(|message| message.role == "student")
            .count(),
    )?;
    // Personal conversation: activity_id and classroom_id stay NULL. The
    // enrollment above only grants entry to the existing student UI.
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO conversations
           (id, user_id, topic_id, title, description, language, status,
            stage, turn_count, converge_ready, ended_at)
         VALUES ($1, $2, $3, $4, $5, $6, 'ended', 3, $7, true, now())
         ON CONFLICT (id) DO NOTHING RETURNING id",
    )
    .bind(id)
    .bind(student_uuid)
    .bind(topic)
    .bind(&chat.title)
    .bind(&chat.description)
    .bind(&language)
    .bind(count)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_error)?;
    if inserted.is_some() {
        // Sequential inserts preserve server seq order, including student
        // fragments; grouping into analysis rounds belongs to the tree tool.
        for message in &chat.messages {
            sqlx::query(
                "INSERT INTO messages (conversation_id, role, content, question_type)
                 VALUES ($1, $2, $3, $4)",
            )
            .bind(id)
            .bind(&message.role)
            .bind(&message.content)
            .bind(&message.question_type)
            .execute(&mut *tx)
            .await
            .map_err(db_error)?;
        }
    }
    let actual: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM messages m JOIN conversations c ON c.id = m.conversation_id
         WHERE c.id = $1 AND c.user_id = $2",
    )
    .bind(id)
    .bind(student_uuid)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_error)?;
    if usize::try_from(actual)? != chat.messages.len() {
        return Err(invalid("既有匯入資料的訊息筆數不符，已取消本次匯入"));
    }
    tx.commit().await.map_err(db_error)?;
    Ok(id)
}

async fn run() -> Result<()> {
    let mut args = env::args_os().skip(1);
    let path = args.next().map(PathBuf::from).ok_or_else(|| {
        invalid(
            "用法：cargo run --locked --example import_tree_chat -- /path/chat.json [student_id]",
        )
    })?;
    let student = args
        .next()
        .map(|value| value.into_string().map_err(|_| invalid("學號編碼不正確")))
        .transpose()?
        .unwrap_or_else(|| DEFAULT_STUDENT.into());
    if args.next().is_some() {
        return Err(invalid("參數過多，只接受 JSON 路徑與選填學號"));
    }
    let bytes = student.as_bytes();
    if bytes.len() != 9
        || !bytes[..8].iter().all(u8::is_ascii_digit)
        || !bytes[8].is_ascii_uppercase()
    {
        return Err(invalid("學號格式應為八位數字加一個大寫英文字母"));
    }
    let chat = parse(&fs::read_to_string(path)?)?;
    let url = env::var("DATABASE_URL")
        .map_err(|_| invalid("缺少 DATABASE_URL；請先從專案根目錄載入 .env"))?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .map_err(db_error)?;
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .map_err(db_error)?;
    let id = import(&pool, &chat, &student).await?;
    let base = env::var("APP_BASE_URL").unwrap_or_else(|_| "http://localhost:5173".into());
    println!(
        "已匯入／已存在：{} 則訊息；學生 {student}",
        chat.messages.len()
    );
    println!("資料庫：{database}");
    println!("對話 ID：{id}");
    println!(
        "前端網址：{}/conversations/{id}",
        base.trim_end_matches('/')
    );
    println!("未產生一般 AI 學習總結；請測試其下方的「總結論證樹」按鈕。");
    pool.close().await;
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_escaped_text_and_separate_student_messages() {
        let content = "換行\nCRLF\r\nTab\t引號\"反斜線\\字面\\n Unicode 😀";
        let raw = serde_json::json!({"topic_id":"T01","title":"合成題目","messages":[
            {"role":"assistant","content":"為什麼？","question_type":"reason"},
            {"role":"user","content":content},
            {"role":"student","content":"補充","question_type":"ignored"}
        ]});
        let chat = parse(&raw.to_string()).unwrap();
        assert_eq!(chat.messages.len(), 3);
        assert_eq!(chat.messages[0].role, "ai");
        assert_eq!(chat.messages[1].role, "student");
        assert_eq!(chat.messages[1].content, content);
        assert_eq!(chat.messages[0].question_type.as_deref(), Some("reason"));
        assert!(chat.messages[2].question_type.is_none());
    }

    #[test]
    fn rejects_invalid_input_before_database_access() {
        for raw in [
            r#"{}"#,
            r#"{"topic_id":"T","title":"題目","messages":[]}"#,
            r#"{"topic_id":"T","title":"題目","messages":[{"role":"system","content":"x"}]}"#,
            r#"{"topic_id":"T","title":"題目","messages":[{"role":"student","content":" "}]}"#,
            r#"{"topic_id":"T","title":"題目","messages":[{"role":"student","content":"\u0000"}]}"#,
        ] {
            assert!(parse(raw).is_err());
        }
    }

    #[test]
    fn stable_ids_are_repeatable_and_scoped() {
        let id = stable_id("conversation", b"same student and content");
        assert_eq!(id, stable_id("conversation", b"same student and content"));
        assert_ne!(id, stable_id("topic", b"same student and content"));
        assert_ne!(
            id,
            stable_id("conversation", b"different student or content")
        );
    }
}
