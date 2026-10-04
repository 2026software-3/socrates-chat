//! 儀表板（F-15、F-24；規格見 S-02.2、S-02.4、S-04）。
//!
//! - 個人儀表板：只算自己的對話與總結。
//! - 班上論點分布：只給教師看；依 S-02.4 不設最小群體門檻。管理者身分一律拿不到分析結果
//!   （S-02.2：分析結果屬敏感欄位），只看得到固定的 `masked: true`。
//!
//! 主要學派（framework）與 6 維分數、主張（claim）是 AI 在產生總結時給的，
//! 分類方式仍屬暫定（S-04 尚未決定），沒有分類的總結歸入 `unclassified`。

use std::collections::BTreeMap;

use axum::{Json, Router, extract::State, routing::get};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState,
    ai::{FRAMEWORKS, MAX_SCORE, SCHOOLS},
    auth::{RequireStudent, RequireTeacher},
    error::ApiError,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/dashboard/me", get(personal))
        .route("/api/dashboard/class", get(class))
}

const UNCLASSIFIED: &str = "unclassified";

/// 每個分類一個計數，沒有人選的分類也列出（值為 0），前端不需要自己補。
type Counts = BTreeMap<&'static str, i64>;

fn empty_counts(keys: &[&'static str]) -> Counts {
    keys.iter()
        .copied()
        .chain([UNCLASSIFIED])
        .map(|k| (k, 0))
        .collect()
}

fn bump(counts: &mut Counts, keys: &[&'static str], value: Option<&str>) {
    let key = value
        .and_then(|v| keys.iter().copied().find(|k| *k == v))
        .unwrap_or(UNCLASSIFIED);
    *counts.entry(key).or_insert(0) += 1;
}

/// 6 維雷達圖的平均分數（S-04.2，暫定）：只平均有分數的總結；沒有任何分數時各維度為 0、`scored` 為 0。
#[derive(Serialize, Clone)]
struct RadarScores {
    /// 有 6 維分數的總結數
    scored: i64,
    /// 各學派的平均分數（0–5，取到小數第 2 位）
    average: BTreeMap<&'static str, f64>,
    /// 單場分數滿分，前端畫圖用
    max: u8,
    #[serde(skip)]
    sums: [i64; 6],
}

impl RadarScores {
    fn new() -> Self {
        let mut r = Self {
            scored: 0,
            average: BTreeMap::new(),
            max: MAX_SCORE,
            sums: [0; 6],
        };
        r.refresh();
        r
    }

    fn add(&mut self, scores: Option<&serde_json::Value>) {
        let Some(obj) = scores.and_then(|v| v.as_object()) else {
            return;
        };
        let mut vals = [0i64; 6];
        for (slot, key) in vals.iter_mut().zip(SCHOOLS) {
            match obj.get(key).and_then(|v| v.as_i64()) {
                Some(n) => *slot = n.clamp(0, i64::from(MAX_SCORE)),
                None => return, // 不完整的分數整組不用
            }
        }
        self.scored += 1;
        for (sum, v) in self.sums.iter_mut().zip(vals) {
            *sum += v;
        }
        self.refresh();
    }

    /// 把另一組分數併進來（把最小的幾個主張群併成「其他」時用）。
    fn merge(&mut self, other: &RadarScores) {
        self.scored += other.scored;
        for (a, b) in self.sums.iter_mut().zip(other.sums) {
            *a += b;
        }
        self.refresh();
    }

    fn refresh(&mut self) {
        for (key, sum) in SCHOOLS.iter().zip(self.sums) {
            let avg = if self.scored > 0 {
                (sum as f64 / self.scored as f64 * 100.0).round() / 100.0
            } else {
                0.0
            };
            self.average.insert(key, avg);
        }
    }
}

#[derive(Serialize)]
struct Distribution {
    completed: i64,
    frameworks: Counts,
    radar: RadarScores,
}

impl Distribution {
    fn new() -> Self {
        Self {
            completed: 0,
            frameworks: empty_counts(&FRAMEWORKS),
            radar: RadarScores::new(),
        }
    }

    fn add(&mut self, framework: Option<&str>, scores: Option<&serde_json::Value>) {
        self.completed += 1;
        bump(&mut self.frameworks, &FRAMEWORKS, framework);
        self.radar.add(scores);
    }
}

// ---- 個人 ----

#[derive(Serialize, FromRow)]
struct RecentRow {
    conversation_id: Uuid,
    title: String,
    ended_at: Option<DateTime<Utc>>,
    stage: i16,
    turn_count: i32,
    stance: Option<String>,
    /// 學生最終主張的一句短句（AI 寫的，暫定）
    claim: Option<String>,
    framework: Option<String>,
    framework_scores: Option<serde_json::Value>,
}

#[derive(Serialize)]
struct Personal {
    total_conversations: i64,
    total_turns: i64,
    #[serde(flatten)]
    distribution: Distribution,
    /// 最近完成的 10 場，新的在前
    recent: Vec<RecentRow>,
}

async fn personal(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Personal>, ApiError> {
    let (total_conversations, total_turns): (i64, i64) = sqlx::query_as(
        "SELECT count(*), COALESCE(sum(turn_count), 0)::bigint FROM conversations WHERE user_id = $1",
    )
    .bind(cu.user.id)
    .fetch_one(&state.pool)
    .await?;
    let ready: Vec<RecentRow> = sqlx::query_as(
        "SELECT c.id AS conversation_id, c.title, c.ended_at, c.stage, c.turn_count,
                s.stance, s.claim, s.framework, s.framework_scores
         FROM summaries s JOIN conversations c ON c.id = s.conversation_id
         WHERE c.user_id = $1 AND s.status = 'ready'
         ORDER BY c.ended_at DESC NULLS LAST, c.id",
    )
    .bind(cu.user.id)
    .fetch_all(&state.pool)
    .await?;
    let mut distribution = Distribution::new();
    for r in &ready {
        distribution.add(r.framework.as_deref(), r.framework_scores.as_ref());
    }
    Ok(Json(Personal {
        total_conversations,
        total_turns,
        distribution,
        recent: ready.into_iter().take(10).collect(),
    }))
}

// ---- 班上（教師）----

/// 一個主張群組：AI 把意思相近的主張歸成一群並命名（暫定）。
#[derive(Serialize)]
struct ClaimGroupView {
    /// 群組名稱（AI 命名，用對話語言）；`other` 為 `true` 時沒有名稱，前端顯示「其他」
    name: Option<String>,
    other: bool,
    count: i64,
    /// 這群人的核心理由（AI 總結的文字，不含姓名與對話原文），最多 5 則、去重
    reasons: Vec<String>,
    /// 這群人在 6 個學派上的平均分數
    radar: RadarScores,
}

#[derive(Serialize)]
struct TopicDistribution {
    title: String,
    #[serde(flatten)]
    distribution: Distribution,
    /// 主張分布：依人數多到少；超過 6 群時，最小的幾群併成「其他」
    claims: Vec<ClaimGroupView>,
    /// 有主張但還沒歸群的場數（分組失敗或尚未分組），教師可按「重新分組」
    ungrouped: i64,
}

/// 主張群組的上限；超過時把最小的幾群併入「其他」。
const MAX_GROUPS: usize = 6;
/// 每個群組列出的理由數。
const MAX_REASONS: usize = 5;

#[derive(Serialize)]
struct Class {
    /// 管理者身分一律為 `true`：看不到分析結果（S-02.2）
    masked: bool,
    students_total: i64,
    /// 至少完成一場對話的學生數
    students_participating: i64,
    #[serde(flatten)]
    overall: Distribution,
    /// 依題目分組，完成場數多的在前
    topics: Vec<TopicDistribution>,
}

#[derive(FromRow)]
struct ClassRow {
    user_id: Uuid,
    title: String,
    claim: Option<String>,
    reasons: Option<String>,
    group_name: Option<String>,
    framework: Option<String>,
    framework_scores: Option<serde_json::Value>,
}

async fn class(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
) -> Result<Json<Class>, ApiError> {
    let (students_total,): (i64,) = sqlx::query_as("SELECT count(*) FROM enrollments")
        .fetch_one(&state.pool)
        .await?;
    if cu.roles.admin {
        return Ok(Json(Class {
            masked: true,
            students_total,
            students_participating: 0,
            overall: Distribution {
                completed: 0,
                frameworks: Counts::new(),
                radar: RadarScores {
                    scored: 0,
                    average: BTreeMap::new(),
                    max: MAX_SCORE,
                    sums: [0; 6],
                },
            },
            topics: vec![],
        }));
    }
    // 只有學生會有對話；事後被移出名單或改成教師也不影響已完成的對話（S-01.4）
    let rows: Vec<ClassRow> = sqlx::query_as(
        "SELECT c.user_id, c.title, s.claim, s.reasons, g.name AS group_name,
                s.framework, s.framework_scores
         FROM summaries s JOIN conversations c ON c.id = s.conversation_id
         LEFT JOIN claim_groups g ON g.id = s.claim_group_id
         WHERE s.status = 'ready'
         ORDER BY c.ended_at, s.conversation_id",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut overall = Distribution::new();
    let mut by_topic: BTreeMap<String, TopicAcc> = BTreeMap::new();
    let mut participants = std::collections::HashSet::new();
    for r in &rows {
        let (f, sc) = (r.framework.as_deref(), r.framework_scores.as_ref());
        overall.add(f, sc);
        let topic = by_topic.entry(r.title.clone()).or_default();
        topic.distribution.add(f, sc);
        if r.claim.is_some() {
            match &r.group_name {
                Some(name) => topic
                    .groups
                    .entry(name.clone())
                    .or_default()
                    .add(r.reasons.as_deref(), sc),
                None => topic.ungrouped += 1,
            }
        }
        participants.insert(r.user_id);
    }
    let mut topics: Vec<TopicDistribution> = by_topic
        .into_iter()
        .map(|(title, acc)| acc.finish(title))
        .collect();
    topics.sort_by(|a, b| {
        b.distribution
            .completed
            .cmp(&a.distribution.completed)
            .then_with(|| a.title.cmp(&b.title))
    });
    Ok(Json(Class {
        masked: false,
        students_total,
        students_participating: participants.len() as i64,
        overall,
        topics,
    }))
}

/// 一個題目的累積結果。
struct TopicAcc {
    distribution: Distribution,
    groups: BTreeMap<String, GroupAcc>,
    ungrouped: i64,
}

impl Default for TopicAcc {
    fn default() -> Self {
        Self {
            distribution: Distribution::new(),
            groups: BTreeMap::new(),
            ungrouped: 0,
        }
    }
}

struct GroupAcc {
    count: i64,
    reasons: Vec<String>,
    radar: RadarScores,
}

impl Default for GroupAcc {
    fn default() -> Self {
        Self {
            count: 0,
            reasons: Vec::new(),
            radar: RadarScores::new(),
        }
    }
}

impl GroupAcc {
    fn add(&mut self, reasons: Option<&str>, scores: Option<&serde_json::Value>) {
        self.count += 1;
        self.radar.add(scores);
        self.push_reason(reasons);
    }

    fn push_reason(&mut self, reason: Option<&str>) {
        let Some(r) = reason.map(str::trim).filter(|r| !r.is_empty()) else {
            return;
        };
        if self.reasons.len() < MAX_REASONS && !self.reasons.iter().any(|x| x == r) {
            self.reasons.push(r.chars().take(300).collect());
        }
    }
}

impl TopicAcc {
    fn finish(self, title: String) -> TopicDistribution {
        let mut groups: Vec<(String, GroupAcc)> = self.groups.into_iter().collect();
        groups.sort_by(|a, b| b.1.count.cmp(&a.1.count).then_with(|| a.0.cmp(&b.0)));
        let mut claims: Vec<ClaimGroupView> = Vec::new();
        let mut rest = GroupAcc::default();
        for (i, (name, g)) in groups.into_iter().enumerate() {
            if i < MAX_GROUPS {
                claims.push(ClaimGroupView {
                    name: Some(name),
                    other: false,
                    count: g.count,
                    reasons: g.reasons,
                    radar: g.radar,
                });
            } else {
                rest.count += g.count;
                rest.radar.merge(&g.radar);
                for r in g.reasons {
                    rest.push_reason(Some(&r));
                }
            }
        }
        if rest.count > 0 {
            claims.push(ClaimGroupView {
                name: None,
                other: true,
                count: rest.count,
                reasons: rest.reasons,
                radar: rest.radar,
            });
        }
        TopicDistribution {
            title,
            distribution: self.distribution,
            claims,
            ungrouped: self.ungrouped,
        }
    }
}
