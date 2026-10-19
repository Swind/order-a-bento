//! SQLite repositories for group orders, append-only submissions and the
//! effective-order read model.
//!
//! Writes run on the single-connection write pool inside `BEGIN IMMEDIATE`
//! transactions (see `directory_repo` for the rationale). Submissions are only
//! ever inserted; the participant-open check is part of the INSERT statement
//! itself so a concurrent close can never be overtaken.

use std::collections::HashSet;
use std::time::SystemTime;

use application::ports::{
    AppendOutcome, ClientView, EffectiveOrderView, GroupListItem, GroupOrderMutation,
    GroupOrderRepo, ItemStat, ManagementReadModel, OrderLineView, OrderReadModel, ParticipantLine,
    ParticipantRow, PortError, PortResult, RepoError, RepoResult, SettlementRepo,
    SubmissionHistoryEntry, SubmissionRepo,
};
use async_trait::async_trait;
use domain::identity::OrganizerId;
use domain::ordering::{
    Actor, ClientInfo, GroupOrder, GroupOrderId, GroupOrderParts, GroupOrderStatus, GroupTitle,
    OfferedItem, OfferedItemId, OrderSubmission, Participant, ParticipantId, ShareToken,
    SubmissionId, SubmissionLine, VendorSnapshot,
};
use domain::settlement::Settlement;
use domain::shared::{ItemName, Money, PersonName};
use sqlx::SqliteConnection;
use uuid::Uuid;

use crate::sqlite::Db;
use crate::timefmt::{from_text, to_text};

fn port(e: impl std::fmt::Display) -> PortError {
    PortError::new(e)
}

fn repo(e: sqlx::Error) -> RepoError {
    if e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
    {
        RepoError::UniqueViolation
    } else {
        RepoError::Port(port(e))
    }
}

fn ts(s: &str) -> PortResult<SystemTime> {
    from_text(s).map_err(port)
}

// ===========================================================================
// Group orders
// ===========================================================================

#[derive(Clone, Debug)]
pub struct SqliteGroupOrderRepo {
    db: Db,
}

impl SqliteGroupOrderRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[derive(sqlx::FromRow)]
struct GroupRow {
    id: String,
    organizer_id: String,
    title: String,
    share_token: String,
    status: String,
    deadline_at: Option<String>,
    note: Option<String>,
    delivery_address: Option<String>,
    source_roster_id: Option<String>,
    source_vendor_id: Option<String>,
    vendor_name: String,
    vendor_phone: Option<String>,
    vendor_address: Option<String>,
    created_at: String,
    closed_at: Option<String>,
}

const GROUP_COLUMNS: &str = "id, organizer_id, title, share_token, status, deadline_at, note, \
     source_roster_id, source_vendor_id, vendor_name, vendor_phone, vendor_address, \
     created_at, closed_at, delivery_address";

async fn assemble_group(conn: &mut SqliteConnection, row: GroupRow) -> PortResult<GroupOrder> {
    let participants = sqlx::query_as::<_, (String, String, i32)>(
        "SELECT id, name, sort_order FROM participants WHERE group_order_id = ? \
         ORDER BY sort_order, id",
    )
    .bind(&row.id)
    .fetch_all(&mut *conn)
    .await
    .map_err(port)?
    .into_iter()
    .map(|(id, name, sort_order)| {
        Ok(Participant {
            id: id.parse().map_err(port)?,
            name: PersonName::new(&name).map_err(port)?,
            sort_order,
        })
    })
    .collect::<PortResult<Vec<_>>>()?;
    let offered_items = sqlx::query_as::<
        _,
        (
            String,
            String,
            i64,
            Option<String>,
            Option<String>,
            i32,
            bool,
        ),
    >(
        "SELECT id, name, price, description, category, sort_order, sold_out FROM offered_items \
         WHERE group_order_id = ? ORDER BY sort_order, id",
    )
    .bind(&row.id)
    .fetch_all(&mut *conn)
    .await
    .map_err(port)?
    .into_iter()
    .map(
        |(id, name, price, description, category, sort_order, sold_out)| {
            Ok(OfferedItem {
                id: id.parse().map_err(port)?,
                name: ItemName::new(&name).map_err(port)?,
                price: Money::try_new(price).map_err(port)?,
                description,
                category,
                sort_order,
                sold_out,
            })
        },
    )
    .collect::<PortResult<Vec<_>>>()?;
    GroupOrder::from_parts(GroupOrderParts {
        id: row.id.parse().map_err(port)?,
        organizer_id: row.organizer_id.parse().map_err(port)?,
        title: GroupTitle::new(&row.title).map_err(port)?,
        share_token: ShareToken::parse(&row.share_token).map_err(port)?,
        status: GroupOrderStatus::parse(&row.status).map_err(port)?,
        deadline_at: row.deadline_at.as_deref().map(ts).transpose()?,
        note: row.note,
        delivery_address: row.delivery_address,
        source_roster_id: row
            .source_roster_id
            .as_deref()
            .map(|v| v.parse().map_err(port))
            .transpose()?,
        source_vendor_id: row
            .source_vendor_id
            .as_deref()
            .map(|v| v.parse().map_err(port))
            .transpose()?,
        vendor: VendorSnapshot {
            name: row.vendor_name,
            phone: row.vendor_phone,
            address: row.vendor_address,
        },
        created_at: ts(&row.created_at)?,
        closed_at: row.closed_at.as_deref().map(ts).transpose()?,
        participants,
        offered_items,
    })
    .map_err(port)
}

async fn load_group_by_id(
    conn: &mut SqliteConnection,
    org: OrganizerId,
    id: GroupOrderId,
) -> PortResult<Option<GroupOrder>> {
    let row = sqlx::query_as::<_, GroupRow>(sqlx::AssertSqlSafe(format!(
        "SELECT {GROUP_COLUMNS} FROM group_orders WHERE id = ? AND organizer_id = ?"
    )))
    .bind(id.to_string())
    .bind(org.to_string())
    .fetch_optional(&mut *conn)
    .await
    .map_err(port)?;
    match row {
        Some(r) => Ok(Some(assemble_group(conn, r).await?)),
        None => Ok(None),
    }
}

async fn insert_participant(
    conn: &mut SqliteConnection,
    group: GroupOrderId,
    p: &Participant,
) -> RepoResult<()> {
    sqlx::query(
        "INSERT INTO participants (id, group_order_id, name, sort_order) VALUES (?, ?, ?, ?)",
    )
    .bind(p.id.to_string())
    .bind(group.to_string())
    .bind(p.name.as_str())
    .bind(p.sort_order)
    .execute(conn)
    .await
    .map_err(repo)?;
    Ok(())
}

#[async_trait]
impl GroupOrderRepo for SqliteGroupOrderRepo {
    async fn create(&self, g: &GroupOrder) -> RepoResult<()> {
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO group_orders ({GROUP_COLUMNS}) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )))
        .bind(g.id().to_string())
        .bind(g.organizer_id().to_string())
        .bind(g.title().as_str())
        .bind(g.share_token().as_str())
        .bind(g.status().as_str())
        .bind(g.deadline_at().map(to_text))
        .bind(g.note())
        .bind(g.source_roster_id().map(|i| i.to_string()))
        .bind(g.source_vendor_id().map(|i| i.to_string()))
        .bind(&g.vendor().name)
        .bind(&g.vendor().phone)
        .bind(&g.vendor().address)
        .bind(to_text(g.created_at()))
        .bind(g.closed_at().map(to_text))
        .bind(g.delivery_address())
        .execute(&mut *tx)
        .await
        .map_err(repo)?;
        for p in g.participants() {
            insert_participant(&mut tx, g.id(), p).await?;
        }
        for i in g.offered_items() {
            sqlx::query(
                "INSERT INTO offered_items \
                   (id, group_order_id, name, price, description, category, sort_order, sold_out) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(i.id.to_string())
            .bind(g.id().to_string())
            .bind(i.name.as_str())
            .bind(i.price.get())
            .bind(&i.description)
            .bind(&i.category)
            .bind(i.sort_order)
            .bind(i.sold_out)
            .execute(&mut *tx)
            .await
            .map_err(repo)?;
        }
        tx.commit().await.map_err(port)?;
        Ok(())
    }

    async fn find_by_id(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
    ) -> PortResult<Option<GroupOrder>> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        load_group_by_id(&mut tx, org, id).await
    }

    async fn find_by_token(&self, token: &ShareToken) -> PortResult<Option<GroupOrder>> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        let row = sqlx::query_as::<_, GroupRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {GROUP_COLUMNS} FROM group_orders WHERE share_token = ?"
        )))
        .bind(token.as_str())
        .fetch_optional(&mut *tx)
        .await
        .map_err(port)?;
        match row {
            Some(r) => Ok(Some(assemble_group(&mut tx, r).await?)),
            None => Ok(None),
        }
    }

    async fn modify(
        &self,
        org: OrganizerId,
        id: GroupOrderId,
        mutation: GroupOrderMutation,
    ) -> RepoResult<GroupOrder> {
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let before = load_group_by_id(&mut tx, org, id)
            .await?
            .ok_or(RepoError::NotFound)?;
        let mut after = before.clone();
        // On error the transaction is dropped, i.e. rolled back.
        mutation(&mut after)?;
        if after == before {
            return Ok(after);
        }
        sqlx::query(
            "UPDATE group_orders SET title = ?, share_token = ?, status = ?, deadline_at = ?, \
               note = ?, closed_at = ?, delivery_address = ? WHERE id = ? AND organizer_id = ?",
        )
        .bind(after.title().as_str())
        .bind(after.share_token().as_str())
        .bind(after.status().as_str())
        .bind(after.deadline_at().map(to_text))
        .bind(after.note())
        .bind(after.closed_at().map(to_text))
        .bind(after.delivery_address())
        .bind(id.to_string())
        .bind(org.to_string())
        .execute(&mut *tx)
        .await
        .map_err(repo)?;
        let known: HashSet<ParticipantId> = before.participants().iter().map(|p| p.id).collect();
        for p in after
            .participants()
            .iter()
            .filter(|p| !known.contains(&p.id))
        {
            insert_participant(&mut tx, id, p).await?;
        }
        for item in after.offered_items() {
            let old = before.offered_items().iter().find(|o| o.id == item.id);
            if old.is_none_or(|o| o.sold_out != item.sold_out) {
                sqlx::query(
                    "UPDATE offered_items SET sold_out = ? WHERE id = ? AND group_order_id = ?",
                )
                .bind(item.sold_out)
                .bind(item.id.to_string())
                .bind(id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(repo)?;
            }
        }
        tx.commit().await.map_err(port)?;
        Ok(after)
    }
}

// ===========================================================================
// Submissions
// ===========================================================================

#[derive(Clone, Debug)]
pub struct SqliteSubmissionRepo {
    db: Db,
}

impl SqliteSubmissionRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[derive(sqlx::FromRow)]
struct SubmissionRow {
    id: String,
    group_order_id: String,
    participant_id: String,
    note: Option<String>,
    idempotency_key: String,
    submitted_by: String,
    submitted_at: String,
    client_ip: Option<String>,
    user_agent: Option<String>,
    ua_summary: Option<String>,
    device_id: Option<String>,
    device_model: Option<String>,
    screen: Option<String>,
    language: Option<String>,
    timezone: Option<String>,
}

const SUBMISSION_COLUMNS: &str = "id, group_order_id, participant_id, note, idempotency_key, \
     submitted_by, submitted_at, client_ip, user_agent, ua_summary, device_id, device_model, \
     screen, language, timezone";

async fn assemble_submission(
    conn: &mut SqliteConnection,
    row: SubmissionRow,
) -> PortResult<OrderSubmission> {
    let lines = sqlx::query_as::<_, (i64, String, i64, i64, Option<String>)>(
        "SELECT line_no, offered_item_id, quantity, unit_price, note FROM submission_lines \
         WHERE submission_id = ? ORDER BY line_no",
    )
    .bind(&row.id)
    .fetch_all(&mut *conn)
    .await
    .map_err(port)?
    .into_iter()
    .map(|(line_no, item, quantity, unit_price, note)| {
        Ok(SubmissionLine {
            line_no: line_no as u32,
            offered_item_id: item.parse().map_err(port)?,
            quantity: quantity as u32,
            unit_price: Money::new(unit_price),
            note,
        })
    })
    .collect::<PortResult<Vec<_>>>()?;
    let client = ClientInfo {
        ip: row.client_ip,
        user_agent: row.user_agent,
        ua_summary: row.ua_summary,
        device_id: row.device_id,
        device_model: row.device_model,
        screen: row.screen,
        language: row.language,
        timezone: row.timezone,
    };
    Ok(OrderSubmission::from_parts(
        row.id.parse().map_err(port)?,
        row.group_order_id.parse().map_err(port)?,
        row.participant_id.parse().map_err(port)?,
        lines,
        row.note,
        Actor::parse(&row.submitted_by).map_err(port)?,
        ts(&row.submitted_at)?,
        Uuid::parse_str(&row.idempotency_key).map_err(port)?,
        (!client.is_empty()).then_some(client),
    ))
}

async fn existing_by_key(
    conn: &mut SqliteConnection,
    group: &str,
    key: &str,
) -> PortResult<Option<SubmissionId>> {
    sqlx::query_as::<_, (String,)>(
        "SELECT id FROM order_submissions WHERE group_order_id = ? AND idempotency_key = ?",
    )
    .bind(group)
    .bind(key)
    .fetch_optional(conn)
    .await
    .map_err(port)?
    .map(|(id,)| id.parse().map_err(port))
    .transpose()
}

#[async_trait]
impl SubmissionRepo for SqliteSubmissionRepo {
    async fn append(
        &self,
        s: &OrderSubmission,
        require_open_for_participant: bool,
        now: SystemTime,
    ) -> RepoResult<AppendOutcome> {
        let group = s.group_order_id().to_string();
        let key = s.idempotency_key().to_string();
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        // (1) idempotent replay.
        if let Some(existing) = existing_by_key(&mut tx, &group, &key).await? {
            return Ok(AppendOutcome::Duplicate(existing));
        }
        // (2) insert only while the group accepts this kind of submission,
        // checked inside the very same statement.
        let condition = if require_open_for_participant {
            "status = 'open' AND (deadline_at IS NULL OR deadline_at > ?)"
        } else {
            "status != 'archived' AND ? IS NOT NULL"
        };
        let ci = s.client_info().cloned().unwrap_or_default();
        let now_text = to_text(now);
        let inserted = sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO order_submissions ({SUBMISSION_COLUMNS}) \
             SELECT ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ? \
             WHERE EXISTS (SELECT 1 FROM group_orders WHERE id = ? AND {condition})"
        )))
        .bind(s.id().to_string())
        .bind(&group)
        .bind(s.participant_id().to_string())
        .bind(s.note())
        .bind(&key)
        .bind(s.submitted_by().as_str())
        .bind(to_text(s.submitted_at()))
        .bind(&ci.ip)
        .bind(&ci.user_agent)
        .bind(&ci.ua_summary)
        .bind(&ci.device_id)
        .bind(&ci.device_model)
        .bind(&ci.screen)
        .bind(&ci.language)
        .bind(&ci.timezone)
        .bind(&group)
        .bind(&now_text)
        .execute(&mut *tx)
        .await;
        let inserted = match inserted {
            Ok(r) => r,
            Err(e)
                if e.as_database_error()
                    .is_some_and(|d| d.is_unique_violation()) =>
            {
                // (4) lost a race on the idempotency key.
                tx.rollback().await.map_err(port)?;
                let mut conn = self.db.write.acquire().await.map_err(port)?;
                return match existing_by_key(&mut conn, &group, &key).await? {
                    Some(id) => Ok(AppendOutcome::Duplicate(id)),
                    None => Err(RepoError::UniqueViolation),
                };
            }
            Err(e) => return Err(repo(e)),
        };
        if inserted.rows_affected() == 0 {
            let row =
                sqlx::query_as::<_, (String,)>("SELECT status FROM group_orders WHERE id = ?")
                    .bind(&group)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(port)?;
            let reason = match row.as_ref().map(|r| r.0.as_str()) {
                Some("open") => "deadline_passed",
                Some("archived") if !require_open_for_participant => "group_archived",
                _ => "group_closed",
            };
            return Ok(AppendOutcome::Rejected(reason));
        }
        // (3) lines.
        for l in s.lines() {
            sqlx::query(
                "INSERT INTO submission_lines \
                   (submission_id, line_no, offered_item_id, quantity, unit_price, note) \
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(s.id().to_string())
            .bind(i64::from(l.line_no))
            .bind(l.offered_item_id.to_string())
            .bind(i64::from(l.quantity))
            .bind(l.unit_price.get())
            .bind(&l.note)
            .execute(&mut *tx)
            .await
            .map_err(repo)?;
        }
        tx.commit().await.map_err(port)?;
        Ok(AppendOutcome::Inserted)
    }

    async fn find_id_by_idempotency_key(
        &self,
        group: GroupOrderId,
        key: Uuid,
    ) -> PortResult<Option<SubmissionId>> {
        let mut conn = self.db.read.acquire().await.map_err(port)?;
        existing_by_key(&mut conn, &group.to_string(), &key.to_string()).await
    }

    async fn find_latest(
        &self,
        group: GroupOrderId,
        participant: ParticipantId,
    ) -> PortResult<Option<OrderSubmission>> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        let row = sqlx::query_as::<_, SubmissionRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {SUBMISSION_COLUMNS} FROM order_submissions \
             WHERE group_order_id = ? AND participant_id = ? ORDER BY id DESC LIMIT 1"
        )))
        .bind(group.to_string())
        .bind(participant.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(port)?;
        match row {
            Some(r) => Ok(Some(assemble_submission(&mut tx, r).await?)),
            None => Ok(None),
        }
    }

    async fn list_for_participant(
        &self,
        group: GroupOrderId,
        participant: ParticipantId,
    ) -> PortResult<Vec<OrderSubmission>> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        let rows = sqlx::query_as::<_, SubmissionRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {SUBMISSION_COLUMNS} FROM order_submissions \
             WHERE group_order_id = ? AND participant_id = ? ORDER BY id DESC"
        )))
        .bind(group.to_string())
        .bind(participant.to_string())
        .fetch_all(&mut *tx)
        .await
        .map_err(port)?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            out.push(assemble_submission(&mut tx, r).await?);
        }
        Ok(out)
    }
}

// ===========================================================================
// Read model
// ===========================================================================

#[derive(Clone, Debug)]
pub struct SqliteOrderReadModel {
    db: Db,
}

impl SqliteOrderReadModel {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl OrderReadModel for SqliteOrderReadModel {
    async fn ordered_participants(
        &self,
        group: GroupOrderId,
    ) -> PortResult<HashSet<ParticipantId>> {
        sqlx::query_as::<_, (String,)>(
            "SELECT es.participant_id FROM effective_submissions es \
             WHERE es.group_order_id = ? \
               AND EXISTS (SELECT 1 FROM submission_lines l WHERE l.submission_id = es.id)",
        )
        .bind(group.to_string())
        .fetch_all(&self.db.read)
        .await
        .map_err(port)?
        .into_iter()
        .map(|(id,)| id.parse().map_err(port))
        .collect()
    }

    async fn effective_order(
        &self,
        group: GroupOrderId,
        participant: ParticipantId,
    ) -> PortResult<Option<EffectiveOrderView>> {
        let mut tx = self.db.read.begin().await.map_err(port)?;
        let Some((id, submitted_at, note)) = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT id, submitted_at, note FROM effective_submissions \
             WHERE group_order_id = ? AND participant_id = ?",
        )
        .bind(group.to_string())
        .bind(participant.to_string())
        .fetch_optional(&mut *tx)
        .await
        .map_err(port)?
        else {
            return Ok(None);
        };
        let lines = sqlx::query_as::<_, (String, String, i64, i64, Option<String>)>(
            "SELECT l.offered_item_id, oi.name, l.quantity, l.unit_price, l.note \
             FROM submission_lines l JOIN offered_items oi ON oi.id = l.offered_item_id \
             WHERE l.submission_id = ? ORDER BY l.line_no",
        )
        .bind(&id)
        .fetch_all(&mut *tx)
        .await
        .map_err(port)?
        .into_iter()
        .map(|(item, name, quantity, unit_price, note)| {
            let unit_price = Money::new(unit_price);
            Ok(OrderLineView {
                item_id: item.parse::<OfferedItemId>().map_err(port)?,
                name,
                quantity: quantity as u32,
                unit_price,
                subtotal: Money::new(unit_price.get() * quantity),
                note,
            })
        })
        .collect::<PortResult<Vec<_>>>()?;
        let total = lines.iter().map(|l| l.subtotal).sum();
        Ok(Some(EffectiveOrderView {
            submission_id: id.parse().map_err(port)?,
            submitted_at: ts(&submitted_at)?,
            note,
            cancelled: lines.is_empty(),
            total,
            lines,
        }))
    }
}

// ===========================================================================
// Management read models
// ===========================================================================

/// First 3 characters of a device id (what the UI shows).
fn device_short(id: &str) -> String {
    id.chars().take(3).collect()
}

fn opt_ts(s: Option<String>) -> PortResult<Option<SystemTime>> {
    s.as_deref().map(ts).transpose()
}

#[derive(sqlx::FromRow)]
struct GroupListRow {
    id: String,
    title: String,
    status: String,
    vendor_name: String,
    deadline_at: Option<String>,
    created_at: String,
    closed_at: Option<String>,
    participant_count: i64,
    ordered_count: i64,
    total_amount: i64,
}

#[derive(sqlx::FromRow)]
struct ParticipantRowRaw {
    id: String,
    name: String,
    sort_order: i64,
    submitted_at: Option<String>,
    order_note: Option<String>,
    device_id: Option<String>,
    amount_due: i64,
    submission_count: i64,
    picked_up_at: Option<String>,
    paid_at: Option<String>,
    paid_amount: Option<i64>,
    suspicious: bool,
}

#[derive(sqlx::FromRow)]
struct HistoryRow {
    id: String,
    submitted_by: String,
    submitted_at: String,
    note: Option<String>,
    client_ip: Option<String>,
    ua_summary: Option<String>,
    device_id: Option<String>,
    device_model: Option<String>,
    screen: Option<String>,
    language: Option<String>,
    timezone: Option<String>,
}

#[async_trait]
impl ManagementReadModel for SqliteOrderReadModel {
    async fn list_groups(
        &self,
        org: OrganizerId,
        status: Option<GroupOrderStatus>,
    ) -> PortResult<Vec<GroupListItem>> {
        let rows = sqlx::query_as::<_, GroupListRow>(
            "SELECT g.id, g.title, g.status, g.vendor_name, g.deadline_at, g.created_at, g.closed_at, \
               (SELECT COUNT(*) FROM participants p WHERE p.group_order_id = g.id) AS participant_count, \
               (SELECT COUNT(*) FROM effective_submissions es WHERE es.group_order_id = g.id \
                  AND EXISTS (SELECT 1 FROM submission_lines l WHERE l.submission_id = es.id)) AS ordered_count, \
               (SELECT COALESCE(SUM(l.quantity * l.unit_price), 0) FROM effective_submissions es \
                  JOIN submission_lines l ON l.submission_id = es.id \
                  WHERE es.group_order_id = g.id) AS total_amount \
             FROM group_orders g \
             WHERE g.organizer_id = ?1 AND (?2 IS NULL OR g.status = ?2) \
             ORDER BY g.created_at DESC, g.id DESC",
        )
        .bind(org.to_string())
        .bind(status.map(|s| s.as_str()))
        .fetch_all(&self.db.read)
        .await
        .map_err(port)?;
        rows.into_iter()
            .map(|r| {
                Ok(GroupListItem {
                    id: r.id.parse().map_err(port)?,
                    title: r.title,
                    status: GroupOrderStatus::parse(&r.status).map_err(port)?,
                    vendor_name: r.vendor_name,
                    deadline_at: opt_ts(r.deadline_at)?,
                    created_at: ts(&r.created_at)?,
                    closed_at: opt_ts(r.closed_at)?,
                    participant_count: r.participant_count as u32,
                    ordered_count: r.ordered_count as u32,
                    total_amount: Money::new(r.total_amount),
                })
            })
            .collect()
    }

    async fn item_stats(&self, group: GroupOrderId) -> PortResult<Vec<ItemStat>> {
        sqlx::query_as::<_, (String, String, i64, i64, i64)>(
            "SELECT oi.id, oi.name, oi.price, \
                    COALESCE(SUM(l.quantity), 0), COALESCE(SUM(l.quantity * l.unit_price), 0) \
             FROM offered_items oi \
             LEFT JOIN submission_lines l ON l.offered_item_id = oi.id \
                  AND l.submission_id IN (SELECT id FROM effective_submissions WHERE group_order_id = ?1) \
             WHERE oi.group_order_id = ?1 \
             GROUP BY oi.id ORDER BY oi.sort_order, oi.id",
        )
        .bind(group.to_string())
        .fetch_all(&self.db.read)
        .await
        .map_err(port)?
        .into_iter()
        .map(|(id, name, price, quantity, subtotal)| {
            Ok(ItemStat {
                item_id: id.parse().map_err(port)?,
                name,
                price: Money::new(price),
                quantity: quantity as u32,
                subtotal: Money::new(subtotal),
            })
        })
        .collect()
    }

    async fn participant_rows(
        &self,
        group: GroupOrderId,
        only: Option<ParticipantId>,
    ) -> PortResult<Vec<ParticipantRow>> {
        let g = group.to_string();
        let only = only.map(|p| p.to_string());
        let mut tx = self.db.read.begin().await.map_err(port)?;
        let rows = sqlx::query_as::<_, ParticipantRowRaw>(
            "SELECT p.id, p.name, p.sort_order, es.submitted_at, es.note AS order_note, es.device_id, \
               COALESCE((SELECT SUM(l.quantity * l.unit_price) FROM submission_lines l \
                         WHERE l.submission_id = es.id), 0) AS amount_due, \
               (SELECT COUNT(*) FROM order_submissions s \
                 WHERE s.group_order_id = p.group_order_id AND s.participant_id = p.id) AS submission_count, \
               st.picked_up_at, st.paid_at, st.paid_amount, \
               EXISTS (SELECT 1 FROM order_submissions a \
                         JOIN order_submissions b ON b.group_order_id = a.group_order_id \
                          AND b.device_id = a.device_id AND b.participant_id <> a.participant_id \
                        WHERE a.group_order_id = p.group_order_id AND a.participant_id = p.id \
                          AND a.device_id IS NOT NULL) AS suspicious \
             FROM participants p \
             LEFT JOIN effective_submissions es \
                    ON es.group_order_id = p.group_order_id AND es.participant_id = p.id \
             LEFT JOIN settlements st \
                    ON st.group_order_id = p.group_order_id AND st.participant_id = p.id \
             WHERE p.group_order_id = ?1 AND (?2 IS NULL OR p.id = ?2) \
             ORDER BY p.sort_order, p.id",
        )
        .bind(&g)
        .bind(&only)
        .fetch_all(&mut *tx)
        .await
        .map_err(port)?;
        let lines = sqlx::query_as::<_, (String, String, String, i64, Option<String>)>(
            "SELECT es.participant_id, l.offered_item_id, oi.name, l.quantity, l.note \
             FROM effective_submissions es \
             JOIN submission_lines l ON l.submission_id = es.id \
             JOIN offered_items oi ON oi.id = l.offered_item_id \
             WHERE es.group_order_id = ?1 AND (?2 IS NULL OR es.participant_id = ?2) \
             ORDER BY es.participant_id, l.line_no",
        )
        .bind(&g)
        .bind(&only)
        .fetch_all(&mut *tx)
        .await
        .map_err(port)?;
        let mut by_participant: std::collections::HashMap<String, Vec<ParticipantLine>> =
            std::collections::HashMap::new();
        for (pid, item, name, quantity, note) in lines {
            by_participant
                .entry(pid)
                .or_default()
                .push(ParticipantLine {
                    item_id: item.parse::<OfferedItemId>().map_err(port)?,
                    name,
                    quantity: quantity as u32,
                    note,
                });
        }
        rows.into_iter()
            .map(|r| {
                let pid: ParticipantId = r.id.parse().map_err(port)?;
                Ok(ParticipantRow {
                    id: pid,
                    name: r.name,
                    sort_order: r.sort_order as i32,
                    amount_due: Money::new(r.amount_due),
                    items: by_participant.remove(&r.id).unwrap_or_default(),
                    order_note: r.order_note,
                    submitted_at: opt_ts(r.submitted_at)?,
                    submission_count: r.submission_count as u32,
                    settlement: Settlement::from_parts(
                        group,
                        pid,
                        opt_ts(r.picked_up_at)?,
                        opt_ts(r.paid_at)?,
                        r.paid_amount.map(Money::new),
                    ),
                    suspicious_device: r.suspicious,
                    device_short: r.device_id.as_deref().map(device_short),
                })
            })
            .collect()
    }

    async fn submission_history(
        &self,
        group: GroupOrderId,
        participant: ParticipantId,
    ) -> PortResult<Vec<SubmissionHistoryEntry>> {
        let (g, p) = (group.to_string(), participant.to_string());
        let mut tx = self.db.read.begin().await.map_err(port)?;
        let rows = sqlx::query_as::<_, HistoryRow>(
            "SELECT id, submitted_by, submitted_at, note, client_ip, ua_summary, device_id, \
                    device_model, screen, language, timezone \
             FROM order_submissions WHERE group_order_id = ? AND participant_id = ? \
             ORDER BY id DESC",
        )
        .bind(&g)
        .bind(&p)
        .fetch_all(&mut *tx)
        .await
        .map_err(port)?;
        let lines = sqlx::query_as::<_, (String, String, String, i64, i64, Option<String>)>(
            "SELECT l.submission_id, l.offered_item_id, oi.name, l.quantity, l.unit_price, l.note \
             FROM submission_lines l \
             JOIN order_submissions s ON s.id = l.submission_id \
             JOIN offered_items oi ON oi.id = l.offered_item_id \
             WHERE s.group_order_id = ? AND s.participant_id = ? \
             ORDER BY l.submission_id, l.line_no",
        )
        .bind(&g)
        .bind(&p)
        .fetch_all(&mut *tx)
        .await
        .map_err(port)?;
        let shared = sqlx::query_as::<_, (String, String)>(
            "SELECT DISTINCT a.device_id, q.name \
             FROM order_submissions a \
             JOIN order_submissions b ON b.group_order_id = a.group_order_id \
              AND b.device_id = a.device_id AND b.participant_id <> a.participant_id \
             JOIN participants q ON q.id = b.participant_id \
             WHERE a.group_order_id = ? AND a.participant_id = ? AND a.device_id IS NOT NULL \
             ORDER BY q.sort_order, q.name",
        )
        .bind(&g)
        .bind(&p)
        .fetch_all(&mut *tx)
        .await
        .map_err(port)?;
        let mut lines_by: std::collections::HashMap<String, Vec<OrderLineView>> =
            std::collections::HashMap::new();
        for (sid, item, name, quantity, unit_price, note) in lines {
            let unit_price = Money::new(unit_price);
            lines_by.entry(sid).or_default().push(OrderLineView {
                item_id: item.parse::<OfferedItemId>().map_err(port)?,
                name,
                quantity: quantity as u32,
                unit_price,
                subtotal: Money::new(unit_price.get() * quantity),
                note,
            });
        }
        rows.into_iter()
            .enumerate()
            .map(|(i, r)| {
                let lines = lines_by.remove(&r.id).unwrap_or_default();
                let shared_device_with = r
                    .device_id
                    .as_ref()
                    .map(|d| {
                        shared
                            .iter()
                            .filter(|(sd, _)| sd == d)
                            .map(|(_, n)| n.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(SubmissionHistoryEntry {
                    id: r.id.parse().map_err(port)?,
                    submitted_at: ts(&r.submitted_at)?,
                    submitted_by: Actor::parse(&r.submitted_by).map_err(port)?,
                    cancelled: lines.is_empty(),
                    total: lines.iter().map(|l| l.subtotal).sum(),
                    note: r.note,
                    lines,
                    client: ClientView {
                        ip: r.client_ip,
                        ua_summary: r.ua_summary,
                        device_short: r.device_id.as_deref().map(device_short),
                        device_model: r.device_model,
                        screen: r.screen,
                        language: r.language,
                        timezone: r.timezone,
                    },
                    effective: i == 0,
                    shared_device_with,
                })
            })
            .collect()
    }
}

// ===========================================================================
// Settlement
// ===========================================================================

#[derive(Clone, Debug)]
pub struct SqliteSettlementRepo {
    db: Db,
}

impl SqliteSettlementRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SettlementRepo for SqliteSettlementRepo {
    async fn set(
        &self,
        org: OrganizerId,
        group: GroupOrderId,
        participant: ParticipantId,
        picked_up: bool,
        paid: bool,
        now: SystemTime,
    ) -> RepoResult<()> {
        let (g, p) = (group.to_string(), participant.to_string());
        let mut tx = self
            .db
            .write
            .begin_with("BEGIN IMMEDIATE")
            .await
            .map_err(port)?;
        let known = sqlx::query_as::<_, (i64,)>(
            "SELECT 1 FROM participants p JOIN group_orders g ON g.id = p.group_order_id \
             WHERE g.id = ? AND g.organizer_id = ? AND p.id = ?",
        )
        .bind(&g)
        .bind(org.to_string())
        .bind(&p)
        .fetch_optional(&mut *tx)
        .await
        .map_err(port)?;
        if known.is_none() {
            return Err(RepoError::NotFound);
        }
        let (amount_due,): (i64,) = sqlx::query_as(
            "SELECT COALESCE(SUM(l.quantity * l.unit_price), 0) \
             FROM effective_submissions es JOIN submission_lines l ON l.submission_id = es.id \
             WHERE es.group_order_id = ? AND es.participant_id = ?",
        )
        .bind(&g)
        .bind(&p)
        .fetch_one(&mut *tx)
        .await
        .map_err(port)?;
        let existing = sqlx::query_as::<_, (Option<String>, Option<String>, Option<i64>)>(
            "SELECT picked_up_at, paid_at, paid_amount FROM settlements \
             WHERE group_order_id = ? AND participant_id = ?",
        )
        .bind(&g)
        .bind(&p)
        .fetch_optional(&mut *tx)
        .await
        .map_err(port)?;
        let before = match existing {
            Some((picked, paid_at, amount)) => Settlement::from_parts(
                group,
                participant,
                opt_ts(picked)?,
                opt_ts(paid_at)?,
                amount.map(Money::new),
            ),
            None => Settlement::new(group, participant),
        };
        let mut after = before.clone();
        after.set(picked_up, paid, Money::new(amount_due), now);
        if after == before {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO settlements \
               (group_order_id, participant_id, picked_up_at, paid_at, paid_amount, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?) \
             ON CONFLICT (group_order_id, participant_id) DO UPDATE SET \
               picked_up_at = excluded.picked_up_at, paid_at = excluded.paid_at, \
               paid_amount = excluded.paid_amount, updated_at = excluded.updated_at",
        )
        .bind(&g)
        .bind(&p)
        .bind(after.picked_up_at().map(to_text))
        .bind(after.paid_at().map(to_text))
        .bind(after.paid_amount().map(|m| m.get()))
        .bind(to_text(now))
        .execute(&mut *tx)
        .await
        .map_err(repo)?;
        tx.commit().await.map_err(port)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use application::ports::{Clock, OrganizerRepo, RosterRepo, VendorRepo};
    use application::{OpenGroupCmd, OrderingError, OrderingService, SubmitOrderCmd};
    use domain::directory::{MemberInput, MenuItemInput, Roster, Vendor};
    use domain::identity::Organizer;
    use domain::ordering::LineInput;
    use domain::shared::{DomainError, FixedClock, SystemClock};

    use super::*;
    use crate::directory_repo::{SqliteRosterRepo, SqliteVendorRepo};
    use crate::identity_repo::SqliteOrganizerRepo;
    use crate::sqlite::TempDir;
    use crate::tokens::OsRngTokenGenerator;

    struct Env {
        db: Db,
        _dir: TempDir,
        org: OrganizerId,
        roster: domain::directory::RosterId,
        vendor: domain::directory::VendorId,
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::now() + Duration::from_secs(secs)
    }

    async fn env(participants: usize) -> Env {
        let (db, dir) = Db::open_temp().await.unwrap();
        let org = SqliteOrganizerRepo::new(db.clone())
            .upsert_from_identity(
                Organizer {
                    id: OrganizerId::new(),
                    oidc_issuer: Some("https://id.example".into()),
                    oidc_sub: Some("sub".into()),
                    email: "a@x.test".into(),
                    display_name: "A".into(),
                    avatar_url: None,
                },
                SystemTime::now(),
            )
            .await
            .unwrap()
            .id;
        let mut roster = Roster::new(org, "R").unwrap();
        roster
            .upsert_members(
                (0..participants)
                    .map(|i| MemberInput {
                        name: format!("P{i:02}"),
                        note: None,
                    })
                    .collect(),
            )
            .unwrap();
        let roster = SqliteRosterRepo::new(db.clone())
            .create(roster, SystemTime::now())
            .await
            .unwrap()
            .value
            .id();
        let mut vendor = Vendor::new(org, "V").unwrap();
        vendor
            .upsert_menu_items(
                [("排骨飯", 100), ("雞腿飯", 110), ("素食", 90)]
                    .into_iter()
                    .map(|(n, p)| MenuItemInput {
                        name: n.into(),
                        price: p,
                        description: None,
                        category: None,
                    })
                    .collect(),
            )
            .unwrap();
        let vendor = SqliteVendorRepo::new(db.clone())
            .create(vendor, SystemTime::now())
            .await
            .unwrap()
            .value
            .id();
        Env {
            db,
            _dir: dir,
            org,
            roster,
            vendor,
        }
    }

    fn service(db: &Db, clock: Arc<dyn Clock>) -> OrderingService {
        OrderingService {
            delivery_addresses: Arc::new(
                crate::delivery_address_repo::SqliteDeliveryAddressRepo::new(db.clone()),
            ),
            groups: Arc::new(SqliteGroupOrderRepo::new(db.clone())),
            submissions: Arc::new(SqliteSubmissionRepo::new(db.clone())),
            read_model: Arc::new(SqliteOrderReadModel::new(db.clone())),
            management: Arc::new(SqliteOrderReadModel::new(db.clone())),
            settlements: Arc::new(SqliteSettlementRepo::new(db.clone())),
            rosters: Arc::new(SqliteRosterRepo::new(db.clone())),
            vendors: Arc::new(SqliteVendorRepo::new(db.clone())),
            tokens: Arc::new(OsRngTokenGenerator),
            clock,
            app_base_url: "http://x.test/".into(),
        }
    }

    fn cmd(e: &Env, deadline: Option<SystemTime>) -> OpenGroupCmd {
        OpenGroupCmd {
            delivery_address_id: None,
            title: "午餐".into(),
            roster_id: e.roster,
            vendor_id: e.vendor,
            deadline_at: deadline,
            note: None,
        }
    }

    fn line(group: &GroupOrder, idx: usize, qty: u32) -> LineInput {
        LineInput {
            offered_item_id: group.offered_items()[idx].id,
            quantity: qty,
            note: None,
        }
    }

    fn submit_cmd(
        group: &GroupOrder,
        p: usize,
        lines: Vec<LineInput>,
        key: Uuid,
    ) -> SubmitOrderCmd {
        SubmitOrderCmd {
            share_token: group.share_token().as_str().to_owned(),
            participant_id: group.participants()[p].id,
            lines,
            note: None,
            idempotency_key: key,
            client_info: ClientInfo {
                ip: Some("1.2.3.4".into()),
                ..Default::default()
            },
        }
    }

    async fn count(db: &Db, sql: &'static str) -> i64 {
        sqlx::query_as::<_, (i64,)>(sql)
            .fetch_one(&db.read)
            .await
            .unwrap()
            .0
    }

    #[tokio::test]
    async fn open_snapshots_roster_and_menu() {
        let e = env(3).await;
        // Archive one menu item before opening.
        let vendors = SqliteVendorRepo::new(e.db.clone());
        let v = vendors.find(e.org, e.vendor).await.unwrap().unwrap().value;
        let archived = v.menu()[2].id;
        vendors
            .modify(
                e.org,
                e.vendor,
                SystemTime::now(),
                Box::new(move |v| v.archive_item(archived).map(|_| None)),
            )
            .await
            .unwrap();
        let svc = service(&e.db, Arc::new(SystemClock));
        let g1 = svc.open_group(e.org, cmd(&e, None)).await.unwrap();
        let g2 = svc.open_group(e.org, cmd(&e, None)).await.unwrap();
        assert_eq!(g1.offered_items().len(), 2);
        assert_eq!(g1.participants().len(), 3);
        assert_eq!(g1.share_token().as_str().len(), 43);
        assert_ne!(g1.share_token(), g2.share_token());
        assert_ne!(g1.id(), g2.id());
        assert_eq!(
            svc.share_url(&g1),
            format!("http://x.test/g/{}", g1.share_token().as_str())
        );

        // Roster / menu edits after opening do not change the snapshot.
        SqliteRosterRepo::new(e.db.clone())
            .modify(
                e.org,
                e.roster,
                SystemTime::now(),
                Box::new(|r| {
                    r.upsert_members(vec![MemberInput {
                        name: "New".into(),
                        note: None,
                    }])
                    .map(Some)
                }),
            )
            .await
            .unwrap();
        vendors
            .modify(
                e.org,
                e.vendor,
                SystemTime::now(),
                Box::new(|v| {
                    v.upsert_menu_items(vec![MenuItemInput {
                        name: "排骨飯".into(),
                        price: 999,
                        description: None,
                        category: None,
                    }])
                    .map(Some)
                }),
            )
            .await
            .unwrap();
        let loaded = svc.get_group(e.org, g1.id()).await.unwrap();
        // Everything but `created_at` (stored at second precision) round-trips.
        assert_eq!(loaded.id(), g1.id());
        assert_eq!(loaded.participants(), g1.participants());
        assert_eq!(loaded.offered_items(), g1.offered_items());
        assert_eq!(loaded.vendor(), g1.vendor());
        assert_eq!(loaded.participants().len(), 3);
        assert_eq!(loaded.offered_items()[0].price.get(), 100);
        // By token as well.
        let by_token = SqliteGroupOrderRepo::new(e.db.clone())
            .find_by_token(g1.share_token())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(by_token, loaded);
    }

    #[tokio::test]
    async fn open_is_scoped_to_the_organizer() {
        let e = env(2).await;
        let svc = service(&e.db, Arc::new(SystemClock));
        let other = OrganizerId::new();
        assert!(matches!(
            svc.open_group(other, cmd(&e, None)).await,
            Err(OrderingError::NotFound)
        ));
        let g = svc.open_group(e.org, cmd(&e, None)).await.unwrap();
        assert!(matches!(
            svc.get_group(other, g.id()).await,
            Err(OrderingError::NotFound)
        ));
        // Past deadline is a domain error.
        assert!(matches!(
            svc.open_group(e.org, cmd(&e, Some(SystemTime::UNIX_EPOCH)))
                .await,
            Err(OrderingError::Domain(DomainError::Invalid { .. }))
        ));
    }

    #[tokio::test]
    async fn append_is_idempotent_and_persists_lines() {
        let e = env(2).await;
        let svc = service(&e.db, Arc::new(SystemClock));
        let g = svc.open_group(e.org, cmd(&e, None)).await.unwrap();
        let key = Uuid::now_v7();
        let c = submit_cmd(&g, 0, vec![line(&g, 0, 2), line(&g, 1, 1)], key);
        let v = svc.submit_order(c.clone()).await.unwrap();
        assert_eq!(v.total.get(), 310);
        assert_eq!(v.lines.len(), 2);
        assert_eq!(v.lines[0].unit_price.get(), 100);
        assert!(!v.cancelled);

        // Replay: same effective order, no new row.
        let again = svc.submit_order(c.clone()).await.unwrap();
        assert_eq!(again.submission_id, v.submission_id);
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM order_submissions").await,
            1
        );
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM submission_lines").await,
            2
        );

        // Port-level duplicate.
        let subs = SqliteSubmissionRepo::new(e.db.clone());
        let dup = g
            .new_submission(
                g.participants()[0].id,
                &[line(&g, 2, 1)],
                None,
                Actor::Participant,
                key,
                None,
                SystemTime::now(),
            )
            .unwrap();
        assert_eq!(
            subs.append(&dup, true, SystemTime::now()).await.unwrap(),
            AppendOutcome::Duplicate(v.submission_id)
        );

        // Stored client info and latest lookup.
        let latest = subs
            .find_latest(g.id(), g.participants()[0].id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(latest.id(), v.submission_id);
        assert_eq!(latest.client_info().unwrap().ip.as_deref(), Some("1.2.3.4"));
        assert_eq!(latest.lines().len(), 2);

        // A newer submission replaces it; history keeps both; cancel = empty.
        let v2 = svc
            .submit_order(submit_cmd(&g, 0, vec![line(&g, 2, 3)], Uuid::now_v7()))
            .await
            .unwrap();
        assert!(v2.submission_id > v.submission_id);
        assert_eq!(v2.total.get(), 270);
        let rm = SqliteOrderReadModel::new(e.db.clone());
        assert!(
            rm.ordered_participants(g.id())
                .await
                .unwrap()
                .contains(&g.participants()[0].id)
        );
        let v3 = svc
            .submit_order(submit_cmd(&g, 0, vec![], Uuid::now_v7()))
            .await
            .unwrap();
        assert!(v3.cancelled && v3.lines.is_empty() && v3.total.get() == 0);
        assert!(rm.ordered_participants(g.id()).await.unwrap().is_empty());
        let hist = subs
            .list_for_participant(g.id(), g.participants()[0].id)
            .await
            .unwrap();
        assert_eq!(hist.len(), 3);
        assert_eq!(hist[0].id(), v3.submission_id);
        // Never submitted -> None.
        assert!(
            rm.effective_order(g.id(), g.participants()[1].id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn closed_and_deadline_are_rejected_atomically() {
        let e = env(1).await;
        let deadline = at(3600);
        let svc = service(&e.db, Arc::new(SystemClock));
        let g = svc
            .open_group(e.org, cmd(&e, Some(deadline)))
            .await
            .unwrap();
        let subs = SqliteSubmissionRepo::new(e.db.clone());
        let mk = |now: SystemTime| {
            g.new_submission(
                g.participants()[0].id,
                &[line(&g, 0, 1)],
                None,
                Actor::Participant,
                Uuid::now_v7(),
                None,
                now,
            )
            .unwrap()
        };
        // Built while valid, appended after the deadline.
        let s = mk(SystemTime::now());
        assert_eq!(
            subs.append(&s, true, deadline + Duration::from_secs(1))
                .await
                .unwrap(),
            AppendOutcome::Rejected("deadline_passed")
        );
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM order_submissions").await,
            0
        );
        assert_eq!(
            subs.append(&s, true, SystemTime::now()).await.unwrap(),
            AppendOutcome::Inserted
        );

        // Close the group.
        let groups = SqliteGroupOrderRepo::new(e.db.clone());
        let closed = groups
            .modify(e.org, g.id(), Box::new(|g| g.close(SystemTime::now())))
            .await
            .unwrap();
        assert_eq!(closed.status(), GroupOrderStatus::Closed);
        assert!(closed.closed_at().is_some());
        assert_eq!(
            subs.append(&mk(SystemTime::now()), true, SystemTime::now())
                .await
                .unwrap(),
            AppendOutcome::Rejected("group_closed")
        );
        // Through the service: domain error with the stable code.
        let err = svc
            .submit_order(submit_cmd(&g, 0, vec![line(&g, 0, 1)], Uuid::now_v7()))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            OrderingError::Domain(DomainError::Closed {
                reason: "group_closed"
            })
        ));
        // The organizer may still submit while closed.
        assert_eq!(
            subs.append(&mk(SystemTime::now()), false, SystemTime::now())
                .await
                .unwrap(),
            AppendOutcome::Inserted
        );
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM order_submissions").await,
            2
        );

        // Mutation errors roll back; other organizers cannot modify.
        let err = groups
            .modify(e.org, g.id(), Box::new(|g| g.close(SystemTime::now())))
            .await
            .unwrap_err();
        assert!(matches!(err, RepoError::Domain(_)));
        assert!(matches!(
            groups
                .modify(
                    OrganizerId::new(),
                    g.id(),
                    Box::new(|g| g.reopen(SystemTime::now()))
                )
                .await,
            Err(RepoError::NotFound)
        ));
        let reopened = groups
            .modify(e.org, g.id(), Box::new(|g| g.reopen(SystemTime::now())))
            .await
            .unwrap();
        assert_eq!(reopened.status(), GroupOrderStatus::Open);
        assert!(reopened.closed_at().is_none());
    }

    #[tokio::test]
    async fn deadline_rejection_via_service_clock() {
        let e = env(1).await;
        let deadline = at(60);
        let open = service(&e.db, Arc::new(SystemClock));
        let g = open
            .open_group(e.org, cmd(&e, Some(deadline)))
            .await
            .unwrap();
        let late = service(
            &e.db,
            Arc::new(FixedClock(deadline + Duration::from_secs(5))),
        );
        let err = late
            .submit_order(submit_cmd(&g, 0, vec![line(&g, 0, 1)], Uuid::now_v7()))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            OrderingError::Domain(DomainError::Closed {
                reason: "deadline_passed"
            })
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn twenty_participants_submit_concurrently() {
        let e = env(20).await;
        let svc = Arc::new(service(&e.db, Arc::new(SystemClock)));
        let g = svc.open_group(e.org, cmd(&e, None)).await.unwrap();
        let mut tasks = Vec::new();
        for p in 0..20 {
            let svc = svc.clone();
            let c = submit_cmd(&g, p, vec![line(&g, p % 3, 1)], Uuid::now_v7());
            tasks.push(tokio::spawn(async move { svc.submit_order(c).await }));
        }
        for t in tasks {
            t.await.unwrap().unwrap();
        }
        let rm = SqliteOrderReadModel::new(e.db.clone());
        assert_eq!(rm.ordered_participants(g.id()).await.unwrap().len(), 20);
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM effective_submissions").await,
            20
        );
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM order_submissions").await,
            20
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn same_participant_twice_concurrently_keeps_both_latest_wins() {
        let e = env(1).await;
        let svc = Arc::new(service(&e.db, Arc::new(SystemClock)));
        let g = svc.open_group(e.org, cmd(&e, None)).await.unwrap();
        let a = submit_cmd(&g, 0, vec![line(&g, 0, 1)], Uuid::now_v7());
        let b = submit_cmd(&g, 0, vec![line(&g, 1, 2)], Uuid::now_v7());
        let (s1, s2) = (svc.clone(), svc.clone());
        let (ra, rb) = tokio::join!(
            tokio::spawn(async move { s1.submit_order(a).await }),
            tokio::spawn(async move { s2.submit_order(b).await })
        );
        ra.unwrap().unwrap();
        rb.unwrap().unwrap();
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM order_submissions").await,
            2
        );
        let (max_id,): (String,) = sqlx::query_as("SELECT MAX(id) FROM order_submissions")
            .fetch_one(&e.db.read)
            .await
            .unwrap();
        let eff = SqliteOrderReadModel::new(e.db.clone())
            .effective_order(g.id(), g.participants()[0].id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(eff.submission_id.to_string(), max_id);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn close_racing_with_submissions_is_consistent() {
        let e = env(40).await;
        let svc = service(&e.db, Arc::new(SystemClock));
        let g = svc.open_group(e.org, cmd(&e, None)).await.unwrap();
        let subs = Arc::new(SqliteSubmissionRepo::new(e.db.clone()));
        let spawn_append = |p: usize| {
            // Built (and validated) while the group is still open.
            let s = g
                .new_submission(
                    g.participants()[p].id,
                    &[line(&g, 0, 1)],
                    None,
                    Actor::Participant,
                    Uuid::now_v7(),
                    None,
                    SystemTime::now(),
                )
                .unwrap();
            let subs = subs.clone();
            tokio::spawn(async move { subs.append(&s, true, SystemTime::now()).await })
        };
        // First wave, the close, second wave: the close lands in the middle.
        let mut tasks: Vec<_> = (0..20).map(spawn_append).collect();
        let groups = SqliteGroupOrderRepo::new(e.db.clone());
        let (org, gid) = (e.org, g.id());
        let closer = tokio::spawn(async move {
            groups
                .modify(org, gid, Box::new(|g| g.close(SystemTime::now())))
                .await
        });
        tasks.extend((20..40).map(spawn_append));
        let (mut ok, mut rejected) = (0, 0);
        for t in tasks {
            match t.await.unwrap().unwrap() {
                AppendOutcome::Inserted => ok += 1,
                AppendOutcome::Rejected(reason) => {
                    assert_eq!(reason, "group_closed");
                    rejected += 1;
                }
                AppendOutcome::Duplicate(_) => panic!("unexpected duplicate"),
            }
        }
        closer.await.unwrap().unwrap();
        assert_eq!(ok + rejected, 40);
        // Every accepted submission is stored; no rejected one is.
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM order_submissions").await,
            ok
        );
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM submission_lines").await,
            ok
        );
        // After the close committed, every further append is refused.
        for p in 0..5 {
            let s = g
                .new_submission(
                    g.participants()[p].id,
                    &[line(&g, 1, 1)],
                    None,
                    Actor::Participant,
                    Uuid::now_v7(),
                    None,
                    SystemTime::now(),
                )
                .unwrap();
            assert_eq!(
                subs.append(&s, true, SystemTime::now()).await.unwrap(),
                AppendOutcome::Rejected("group_closed")
            );
        }
        assert_eq!(
            count(&e.db, "SELECT COUNT(*) FROM order_submissions").await,
            ok
        );
    }
}
