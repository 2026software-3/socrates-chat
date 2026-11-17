//! F-24.1-AI (#521): contract of `radar_scoring::score` against a scripted provider.
//! These tests use a test double; they say nothing about a real model's scoring quality
//! (see `backend/eval/f24_1_README.md`).
mod common;

use std::time::Duration;

use common::*;
use socrates_chat_backend::{
    ai::{AiError, ChatRole, ChatTurn},
    radar_scoring::{Dimension, build_request, rubric_for, score},
    radar_store::Scores,
};

fn dimensions() -> Vec<Dimension> {
    (1..=6)
        .map(|n| Dimension {
            id: format!("d{n}"),
            name: format!("Tendency {n}"),
            description: format!("What tendency {n} looks like."),
            rubric: std::array::from_fn(|level| format!("Tendency {n}, level {}", level + 1)),
        })
        .collect()
}

fn turns(student: &str) -> Vec<ChatTurn> {
    vec![
        ChatTurn {
            role: ChatRole::Assistant,
            content: "Would you pull the lever?".into(),
        },
        ChatTurn {
            role: ChatRole::User,
            content: student.into(),
        },
    ]
}

async fn run(ai: &FakeAi, student: &str) -> Result<Scores, AiError> {
    let dimensions = dimensions();
    score(
        ai,
        build_request(
            "Trolley problem",
            "Pull the lever?",
            &dimensions,
            &turns(student),
        ),
        &rubric_for(&dimensions),
        Duration::from_millis(500),
    )
    .await
}

#[tokio::test]
async fn a_complete_answer_becomes_six_scores_keyed_by_dimension_id() {
    // Split across chunks the way a streamed answer arrives.
    let ai = FakeAi::with(vec![Script::Reply(vec![
        r#"{"d1":4,"d2":0,"#,
        r#""d3":3,"d4":0,"d5":1,"d6":5}"#,
    ])]);
    let scores = run(&ai, "Yes, five lives outweigh one.").await.unwrap();
    assert_eq!(
        scores.into_iter().collect::<Vec<_>>(),
        [
            ("d1".to_owned(), 4),
            ("d2".to_owned(), 0),
            ("d3".to_owned(), 3),
            ("d4".to_owned(), 0),
            ("d5".to_owned(), 1),
            ("d6".to_owned(), 5),
        ]
    );
}

#[tokio::test]
async fn an_invalid_answer_is_refused_whole_and_reported_as_invalid_output() {
    for answer in [
        // missing d6
        r#"{"d1":1,"d2":1,"d3":1,"d4":1,"d5":1}"#,
        // repeated d1
        r#"{"d1":1,"d1":2,"d2":1,"d3":1,"d4":1,"d5":1,"d6":1}"#,
        // unknown d7
        r#"{"d1":1,"d2":1,"d3":1,"d4":1,"d5":1,"d6":1,"d7":1}"#,
        // not an integer
        r#"{"d1":2.5,"d2":1,"d3":1,"d4":1,"d5":1,"d6":1}"#,
        // out of range
        r#"{"d1":6,"d2":1,"d3":1,"d4":1,"d5":1,"d6":1}"#,
        // not JSON
        "I cannot score this discussion.",
    ] {
        let ai = FakeAi::with(vec![Script::Reply(vec![answer])]);
        assert_eq!(
            run(&ai, "Yes.").await,
            Err(AiError::invalid_output()),
            "{answer}"
        );
        // One call per attempt: retrying is the caller's decision (S-03.5).
        assert_eq!(ai.request_count(), 1);
    }
}

#[tokio::test]
async fn provider_failures_and_timeouts_are_errors_not_scores() {
    for script in [
        Script::Fail,
        Script::FailAfter(vec![r#"{"d1":1,"d2":1,"d3":1,"#]),
        Script::Hang,
    ] {
        let ai = FakeAi::with(vec![script]);
        assert!(run(&ai, "Yes.").await.is_err());
    }
}

#[tokio::test]
async fn each_request_holds_only_its_own_conversation() {
    const ANSWER: &str = r#"{"d1":1,"d2":1,"d3":1,"d4":1,"d5":1,"d6":1}"#;
    let ai = FakeAi::with(vec![
        Script::Reply(vec![ANSWER]),
        Script::Reply(vec![ANSWER]),
    ]);
    run(&ai, "First student: save the five.").await.unwrap();
    run(&ai, "Second student: never pull it.").await.unwrap();
    let requests = ai.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    let (first, second) = (
        &requests[0].messages[0].content,
        &requests[1].messages[0].content,
    );
    assert!(first.contains("First student") && !first.contains("Second student"));
    assert!(second.contains("Second student") && !second.contains("First student"));
}
