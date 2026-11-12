//! 儀表板（F-15、F-24；規格見 S-02.2、S-02.4、S-04.6、S-04.7）。
//!
//! - 個人儀表板：只算自己的對話與總結。
//! - 教師儀表板：只給教師看；依 S-02.4 不設最小群體門檻。以「活動內的題目」為彙整單位（S-04.7）：
//!   每位學生在活動內每題至多一場對話，從題目庫開始的個人練習不屬於任何活動，不列入。
//!   管理者身分一律拿不到分析結果（S-02.2：分析結果屬敏感欄位），只看得到固定的 `masked: true`。
//!
//! 主張（claim）是 AI 在產生總結時給的，分組方式仍屬暫定。固定學派的 6 維雷達圖已取消（S-04.6），
//! 新的題目綁定維度由後續功能實作。

use std::collections::BTreeMap;

use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    AppState,
    auth::{RequireStudent, RequireTeacher},
    classrooms::{ClassroomFilter, filtered_classroom},
    error::ApiError,
    roster_store,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/dashboard/me", get(personal))
        .route("/api/dashboard/class", get(class))
}

// ---- 個人 ----

#[derive(Serialize, FromRow, utoipa::ToSchema)]
pub(crate) struct RecentRow {
    conversation_id: Uuid,
    title: String,
    ended_at: Option<DateTime<Utc>>,
    stage: i16,
    turn_count: i32,
    stance: Option<String>,
    /// 學生最終主張的一句短句（AI 寫的，暫定）
    claim: Option<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Personal {
    total_conversations: i64,
    total_turns: i64,
    /// 已產生總結的對話數
    completed: i64,
    /// 最近完成的 10 場，新的在前
    recent: Vec<RecentRow>,
}

#[utoipa::path(
    get,
    path = "/api/dashboard/me",
    tag = "dashboard",
    summary = "個人儀表板",
    responses((status = 200, description = "個人統計", body = Personal))
)]
pub(crate) async fn personal(
    RequireStudent(cu): RequireStudent,
    State(state): State<AppState>,
) -> Result<Json<Personal>, ApiError> {
    // 已完成的總數在資料庫算，只取最近 10 筆的內容
    let ((total_conversations, total_turns, completed), recent): ((i64, i64, i64), Vec<RecentRow>) =
        tokio::try_join!(
            sqlx::query_as(
                "SELECT count(*), COALESCE(sum(turn_count), 0)::bigint,
                        (SELECT count(*) FROM summaries s
                         JOIN conversations c ON c.id = s.conversation_id
                         WHERE c.user_id = $1 AND s.status = 'ready')
                 FROM conversations WHERE user_id = $1",
            )
            .bind(cu.user.id)
            .fetch_one(&state.pool),
            sqlx::query_as(
                "SELECT c.id AS conversation_id, c.title, c.ended_at, c.stage, c.turn_count,
                        s.stance, s.claim
                 FROM summaries s JOIN conversations c ON c.id = s.conversation_id
                 WHERE c.user_id = $1 AND s.status = 'ready'
                 ORDER BY c.ended_at DESC NULLS LAST, c.id
                 LIMIT 10",
            )
            .bind(cu.user.id)
            .fetch_all(&state.pool),
        )?;
    Ok(Json(Personal {
        total_conversations,
        total_turns,
        completed,
        recent,
    }))
}

// ---- 班上（教師）----

/// 一個主張群組：AI 把意思相近的主張歸成一群並命名（暫定）。
#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ClaimGroupView {
    /// 群組名稱（AI 命名，用對話語言）；`other` 為 `true` 時沒有名稱，前端顯示「其他」
    name: Option<String>,
    other: bool,
    count: i64,
    /// 這群人的核心理由（AI 總結的文字，不含姓名與對話原文），最多 5 則、去重
    reasons: Vec<String>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct ActivityDistribution {
    activity_id: Uuid,
    title: String,
    /// 已產生總結的對話數，並非完成全部題目的學生數。
    completed: i64,
    /// 依教師設定的題目順序，包含尚無總結的題目。
    topics: Vec<TopicDistribution>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct TopicDistribution {
    topic_id: Uuid,
    title: String,
    /// 本題已產生總結的對話數。
    completed: i64,
    /// 主張群依人數排序，超過六群後併入其他。
    claims: Vec<ClaimGroupView>,
    /// 有主張但尚未分組的場數。
    ungrouped: i64,
}

/// 主張群組的上限；超過時把最小的幾群併入「其他」。
const MAX_GROUPS: usize = 6;
/// 每個群組列出的理由數。
const MAX_REASONS: usize = 5;

#[derive(Serialize, utoipa::ToSchema)]
pub(crate) struct Class {
    /// 管理者身分一律為 `true`：看不到分析結果（S-02.2）
    masked: bool,
    students_total: i64,
    /// 至少完成一場活動對話的學生數
    students_participating: i64,
    /// 全部活動已完成的對話數
    completed: i64,
    /// 依活動分組，完成場數多的在前
    activities: Vec<ActivityDistribution>,
}

#[derive(FromRow)]
struct ClassRow {
    user_id: Uuid,
    activity_id: Uuid,
    topic_id: Option<Uuid>,
    claim: Option<String>,
    reasons: Option<String>,
    group_name: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/dashboard/class",
    tag = "dashboard",
    summary = "各活動的論點分布（只有教師）",
    params(("classroom_id" = Option<Uuid>, Query, description = "只看這間教室；不是自己管理的教室回 404。沒帶時涵蓋自己管理的所有教室")),
    responses((status = 200, description = "班級統計", body = Class), crate::error::CommonErrors)
)]
pub(crate) async fn class(
    RequireTeacher(cu): RequireTeacher,
    State(state): State<AppState>,
    Query(filter): Query<ClassroomFilter>,
) -> Result<Json<Class>, ApiError> {
    let classroom = filtered_classroom(&state, &cu, &filter).await?;
    let students_total = async {
        match classroom {
            Some(id) => roster_store::count_in_classroom(&state.pool, id).await,
            None => roster_store::count_students(&state.pool, cu.user.id, cu.roles.admin).await,
        }
    };
    if cu.roles.admin {
        let students_total = students_total.await?;
        return Ok(Json(Class {
            masked: true,
            students_total,
            students_participating: 0,
            completed: 0,
            activities: vec![],
        }));
    }
    // 只有學生會有對話；事後被移出名單或改成教師也不影響已完成的對話（S-01.4）。
    // 內連接教室與活動：從題目庫開始的個人練習不屬於任何教室，教師看不到，也不納入統計（S-02.1、S-14）。
    let rows = sqlx::query_as::<_, ClassRow>(
        "SELECT c.user_id, c.activity_id, c.topic_id,
                s.claim, s.reasons, g.name AS group_name
         FROM summaries s
         JOIN conversations c ON c.id = s.conversation_id
         JOIN classrooms cl ON cl.id = c.classroom_id AND ($1 OR cl.created_by = $2)
         JOIN activities a ON a.id = c.activity_id
         LEFT JOIN claim_groups g ON g.id = s.claim_group_id
              AND g.activity_id = c.activity_id AND g.topic_id = c.topic_id
         WHERE s.status = 'ready' AND ($3::uuid IS NULL OR c.classroom_id = $3)
         ORDER BY c.ended_at, s.conversation_id",
    )
    .bind(cu.roles.admin)
    .bind(cu.user.id)
    .bind(classroom)
    .fetch_all(&state.pool);
    let topic_rows = sqlx::query_as::<_, (Uuid, String, Uuid, String)>(
        "SELECT a.id, a.title, t.id, t.title
         FROM activities a JOIN classrooms cl ON cl.id = a.classroom_id
         JOIN activity_topics at ON at.activity_id = a.id
         JOIN topics t ON t.id = at.topic_id
         WHERE cl.created_by = $1 AND ($2::uuid IS NULL OR cl.id = $2)
         ORDER BY a.id, at.position",
    )
    .bind(cu.user.id)
    .bind(classroom)
    .fetch_all(&state.pool);
    let (students_total, rows, topic_rows) = tokio::try_join!(students_total, rows, topic_rows)?;
    let mut by_topic: BTreeMap<(Uuid, Uuid), TopicAcc> = BTreeMap::new();
    for (activity, _, topic, title) in &topic_rows {
        by_topic.insert(
            (*activity, *topic),
            TopicAcc {
                title: title.clone(),
                completed: 0,
                groups: BTreeMap::new(),
                ungrouped: 0,
            },
        );
    }
    let mut participants = std::collections::HashSet::new();
    for r in &rows {
        participants.insert(r.user_id);
        let Some(topic) = r
            .topic_id
            .and_then(|id| by_topic.get_mut(&(r.activity_id, id)))
        else {
            continue;
        };
        topic.completed += 1;
        if r.claim.is_some() {
            match &r.group_name {
                Some(name) => topic
                    .groups
                    .entry(name.clone())
                    .or_default()
                    .add(r.reasons.as_deref()),
                None => topic.ungrouped += 1,
            }
        }
    }
    let mut by_activity: BTreeMap<Uuid, ActivityDistribution> = BTreeMap::new();
    for (id, title, topic_id, _) in topic_rows {
        let topic = by_topic
            .remove(&(id, topic_id))
            .expect("activity topic initialized")
            .finish(topic_id);
        let activity = by_activity
            .entry(id)
            .or_insert_with(|| ActivityDistribution {
                activity_id: id,
                title,
                completed: 0,
                topics: vec![],
            });
        activity.completed += topic.completed;
        activity.topics.push(topic);
    }
    let mut activities: Vec<_> = by_activity.into_values().collect();
    activities.sort_by(|a, b| {
        b.completed
            .cmp(&a.completed)
            .then_with(|| a.title.cmp(&b.title))
            .then_with(|| a.activity_id.cmp(&b.activity_id))
    });
    Ok(Json(Class {
        masked: false,
        students_total,
        students_participating: participants.len() as i64,
        completed: rows.len() as i64,
        activities,
    }))
}

/// 活動內單一題目的累積結果。
struct TopicAcc {
    title: String,
    completed: i64,
    groups: BTreeMap<String, GroupAcc>,
    ungrouped: i64,
}

#[derive(Default)]
struct GroupAcc {
    count: i64,
    reasons: Vec<String>,
}

impl GroupAcc {
    fn add(&mut self, reasons: Option<&str>) {
        self.count += 1;
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
    fn finish(self, topic_id: Uuid) -> TopicDistribution {
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
                });
            } else {
                rest.count += g.count;
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
            });
        }
        TopicDistribution {
            topic_id,
            title: self.title,
            completed: self.completed,
            claims,
            ungrouped: self.ungrouped,
        }
    }
}
