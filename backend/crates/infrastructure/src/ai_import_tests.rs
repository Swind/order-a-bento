//! Tests of the AI import tools and service against real SQLite repositories.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use application::ai_import::tools::{
    self, GetRosterArgs, GetRosterOutput, GetVendorArgs, GetVendorOutput, ProposeRosterMembersArgs,
    ProposeVendorArgs,
};
use application::ai_import::{
    AcceptedChange, AcceptedMember, AcceptedMenuItem, AcceptedRoster, AcceptedVendor,
    AcceptedVendorInfo, AgentInput, AgentOutcome, AiError, AiImportConfig, AiImportError,
    AiImportService, ApplyCmd, FakeImportAgent, FakeToolCall, ImageInput, ImportAgent,
    ImportToolContext, StartImportCmd,
};
use application::ports::{OrganizerRepo, RosterRepo, VendorRepo};
use async_trait::async_trait;
use domain::ai_import::{
    ChangeAction, ImportMode, ImportRunStatus, Issue, ProposalTarget, TargetRef, Usage,
};
use domain::directory::{MemberInput, MenuItemInput, Roster, RosterId, Vendor, VendorId};
use domain::identity::{Organizer, OrganizerId};
use domain::shared::SystemClock;
use serde_json::json;

use crate::ai_import_repo::SqliteImportRunRepo;
use crate::directory_repo::{SqliteRosterRepo, SqliteVendorRepo};
use crate::identity_repo::SqliteOrganizerRepo;
use crate::sqlite::{Db, TempDir};

struct Env {
    db: Db,
    _dir: TempDir,
    org: OrganizerId,
    other: OrganizerId,
    rosters: Arc<SqliteRosterRepo>,
    vendors: Arc<SqliteVendorRepo>,
    runs: Arc<SqliteImportRunRepo>,
}

async fn organizer(db: &Db, n: &str) -> OrganizerId {
    let o = Organizer::new(
        "https://id.example",
        &format!("sub-{n}"),
        &format!("{n}@x.com"),
        n,
        None,
    )
    .unwrap();
    SqliteOrganizerRepo::new(db.clone())
        .upsert_from_identity(o, SystemTime::now())
        .await
        .unwrap()
        .id
}

async fn env() -> Env {
    let (db, dir) = Db::open_temp().await.unwrap();
    let org = organizer(&db, "a").await;
    let other = organizer(&db, "b").await;
    Env {
        rosters: Arc::new(SqliteRosterRepo::new(db.clone())),
        vendors: Arc::new(SqliteVendorRepo::new(db.clone())),
        runs: Arc::new(SqliteImportRunRepo::new(db.clone())),
        db,
        _dir: dir,
        org,
        other,
    }
}

impl Env {
    fn ctx(&self, org: OrganizerId) -> ImportToolContext {
        ImportToolContext::new(
            org,
            domain::ai_import::ImportRunId::new(),
            self.rosters.clone(),
            self.vendors.clone(),
            Arc::new(SystemClock),
        )
    }

    fn service(&self, agent: Arc<dyn ImportAgent>, quota: u32) -> AiImportService {
        let mut config = AiImportConfig::new("test-model", quota);
        config.timeout = Duration::from_millis(300);
        config.retry_delay = Duration::from_millis(10);
        AiImportService {
            runs: self.runs.clone(),
            apply_repo: self.runs.clone(),
            rosters: self.rosters.clone(),
            vendors: self.vendors.clone(),
            agent,
            clock: Arc::new(SystemClock),
            config,
        }
    }

    async fn roster(
        &self,
        org: OrganizerId,
        name: &str,
        members: &[(&str, Option<&str>)],
    ) -> RosterId {
        let mut r = Roster::new(org, name).unwrap();
        r.upsert_members(
            members
                .iter()
                .map(|(n, note)| MemberInput {
                    name: (*n).into(),
                    note: note.map(Into::into),
                })
                .collect(),
        )
        .unwrap();
        let id = r.id();
        self.rosters.create(r, SystemTime::now()).await.unwrap();
        id
    }

    async fn vendor(
        &self,
        org: OrganizerId,
        name: &str,
        phone: Option<&str>,
        items: &[(&str, i64)],
    ) -> VendorId {
        let mut v = Vendor::new(org, name).unwrap();
        v.update_info(domain::directory::VendorInfoPatch {
            phone: phone.map(Into::into),
            ..Default::default()
        })
        .unwrap();
        v.upsert_menu_items(
            items
                .iter()
                .map(|(n, p)| MenuItemInput {
                    name: (*n).into(),
                    price: *p,
                    description: None,
                    category: None,
                })
                .collect(),
        )
        .unwrap();
        let id = v.id();
        self.vendors.create(v, SystemTime::now()).await.unwrap();
        id
    }

    async fn run_status(&self, org: OrganizerId) -> Vec<String> {
        sqlx::query_as::<_, (String,)>(
            "SELECT status FROM ai_import_runs WHERE organizer_id = ? ORDER BY created_at, id",
        )
        .bind(org.to_string())
        .fetch_all(&self.db.read)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.0)
        .collect()
    }
}

fn rargs(v: serde_json::Value) -> ProposeRosterMembersArgs {
    serde_json::from_value(v).unwrap()
}
fn vargs(v: serde_json::Value) -> ProposeVendorArgs {
    serde_json::from_value(v).unwrap()
}
fn cmd() -> StartImportCmd {
    StartImportCmd {
        pending_photo: None,
        sharing_group_id: None,
        image: ImageInput {
            bytes: vec![1, 2, 3],
            media_type: "image/jpeg".into(),
        },
        instruction: None,
        mode: ImportMode::Roster,
        target_roster_id: None,
        target_vendor_id: None,
    }
}

// ===========================================================================
// read tools
// ===========================================================================

#[tokio::test]
async fn read_tools_are_scoped_by_organizer() {
    let e = env().await;
    let mine = e
        .roster(e.org, "我的", &[("A", Some("n")), ("B", None)])
        .await;
    let theirs = e.roster(e.other, "別人的", &[("X", None)]).await;
    let my_vendor = e.vendor(e.org, "我的店", Some("02-1"), &[("飯", 80)]).await;
    let their_vendor = e.vendor(e.other, "別人的店", None, &[("麵", 70)]).await;
    let ctx = e.ctx(e.org);

    let rs = tools::list_rosters(&ctx).await.unwrap();
    assert_eq!(rs.len(), 1);
    assert_eq!((rs[0].name.as_str(), rs[0].member_count), ("我的", 2));
    let json = serde_json::to_value(&rs).unwrap();
    assert_eq!(json[0]["memberCount"], 2);

    let GetRosterOutput::Found(d) = tools::get_roster(
        &ctx,
        GetRosterArgs {
            roster_id: mine.to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!()
    };
    assert_eq!(d.members.len(), 2);
    assert_eq!(d.members[0].note.as_deref(), Some("n"));
    // someone else's id, a malformed id: error values, not Err
    for bad in [theirs.to_string(), "nope".to_string()] {
        let out = tools::get_roster(&ctx, GetRosterArgs { roster_id: bad })
            .await
            .unwrap();
        assert!(matches!(out, GetRosterOutput::Error(_)));
    }
    let v = serde_json::to_value(
        tools::get_roster(
            &ctx,
            GetRosterArgs {
                roster_id: theirs.to_string(),
            },
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert!(v["error"].as_str().unwrap().contains("roster not found"));

    let vs = tools::list_vendors(&ctx).await.unwrap();
    assert_eq!(vs.len(), 1);
    assert_eq!(
        (vs[0].item_count, vs[0].phone.as_deref()),
        (1, Some("02-1"))
    );
    let GetVendorOutput::Found(d) = tools::get_vendor(
        &ctx,
        GetVendorArgs {
            vendor_id: my_vendor.to_string(),
        },
    )
    .await
    .unwrap() else {
        panic!()
    };
    assert_eq!((d.menu[0].name.as_str(), d.menu[0].price), ("飯", 80));
    assert!(matches!(
        tools::get_vendor(
            &ctx,
            GetVendorArgs {
                vendor_id: their_vendor.to_string()
            }
        )
        .await
        .unwrap(),
        GetVendorOutput::Error(_)
    ));
}

// ===========================================================================
// propose_roster_members
// ===========================================================================

#[tokio::test]
async fn propose_roster_diffs_against_current_data() {
    let e = env().await;
    let id = e
        .roster(
            e.org,
            "業務",
            &[
                ("Amy", Some("素食")),
                ("王小明", Some("分機1")),
                ("Cat", None),
            ],
        )
        .await;
    let ctx = e.ctx(e.org);
    let out = tools::propose_roster_members(
        &ctx,
        rargs(json!({
            "target": { "rosterId": id.to_string() },
            "members": [
                { "name": "ＡＭＹ", "note": "不吃辣", "confidence": "high" },
                { "name": "王小明 ", "note": "  " },
                { "name": "Cat", "note": "新備註", "confidence": "low" },
                { "name": "新人" },
                { "name": "Dan", "note": "分機9" }
            ],
            "warnings": ["第 3 列模糊"]
        })),
    )
    .await
    .unwrap();
    assert_eq!((out.created, out.updated, out.unchanged), (2, 2, 1));
    assert!(out.errors.is_empty() && out.recorded);

    let p = ctx.snapshot().await;
    assert_eq!(p.warnings, ["第 3 列模糊"]);
    let ProposalTarget::Roster(t) = &p.targets[0] else {
        panic!()
    };
    assert_eq!(t.roster, TargetRef::Existing { existing_id: id });
    let m = &t.members;
    // ＡＭＹ matches Amy: existing display name kept, note updated
    assert_eq!(
        (m[0].action, m[0].name.as_str()),
        (ChangeAction::Update, "Amy")
    );
    assert_eq!(
        (m[0].before_note.as_deref(), m[0].note.as_deref()),
        (Some("素食"), Some("不吃辣"))
    );
    // blank note never overwrites
    assert_eq!(m[1].action, ChangeAction::Unchanged);
    assert_eq!(m[1].note.as_deref(), Some("分機1"));
    assert_eq!(m[2].action, ChangeAction::Update);
    assert_eq!(m[3].action, ChangeAction::Create);
    assert_eq!(m[4].action, ChangeAction::Create);
    assert_eq!(serde_json::to_value(m[2].confidence).unwrap(), "low");

    // the same note again -> unchanged
    let out = tools::propose_roster_members(
        &ctx,
        rargs(json!({ "target": {"rosterId": id.to_string()}, "members": [{"name": "amy", "note": "素食"}] })),
    )
    .await
    .unwrap();
    assert_eq!((out.created, out.updated, out.unchanged), (0, 0, 1));
    // real data untouched
    let r = e.rosters.find(e.org, id).await.unwrap().unwrap().value;
    assert_eq!(r.members().len(), 3);
    assert_eq!(r.members()[0].note.as_deref(), Some("素食"));
}

#[tokio::test]
async fn propose_roster_reports_fixable_errors_but_records_entries() {
    let e = env().await;
    let ctx = e.ctx(e.org);
    let long = "字".repeat(51);
    let out = tools::propose_roster_members(
        &ctx,
        rargs(json!({
            "target": { "newRosterName": "新名單" },
            "members": [
                { "name": "Amy" },
                { "name": "  " },
                { "name": long },
                { "name": "ａｍｙ" },
                { "name": "Bob", "note": "x".repeat(201) }
            ]
        })),
    )
    .await
    .unwrap();
    assert!(out.recorded);
    assert_eq!(out.created, 1);
    let idx: Vec<(Option<usize>, &str)> = out
        .errors
        .iter()
        .map(|x| (x.index, x.field.as_str()))
        .collect();
    assert_eq!(
        idx,
        [
            (Some(1), "name"),
            (Some(2), "name"),
            (Some(3), "name"),
            (Some(4), "note")
        ]
    );
    assert!(out.errors[1].message.contains("at most 50"));
    assert!(out.errors[2].message.contains("entry 1"));
    let p = ctx.snapshot().await;
    let ProposalTarget::Roster(t) = &p.targets[0] else {
        panic!()
    };
    assert_eq!(
        t.roster,
        TargetRef::New {
            name: "新名單".into()
        }
    );
    assert_eq!(t.members.len(), 5);
    assert_eq!(t.members[1].issues, [Issue::NameEmpty]);
    assert_eq!(t.members[2].issues, [Issue::NameTooLong]);
    assert_eq!(t.members[3].issues, [Issue::DuplicateInInput]);
    assert_eq!(t.members[4].issues, [Issue::NoteTooLong]);
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(v["targets"][0]["members"][1]["issues"][0], "name_empty");
}

#[tokio::test]
async fn propose_roster_bad_targets_are_rejected_not_recorded() {
    let e = env().await;
    let theirs = e.roster(e.other, "別人的", &[]).await;
    let ctx = e.ctx(e.org);
    for target in [
        json!({ "rosterId": theirs.to_string() }),
        json!({ "rosterId": "garbage" }),
        json!({ "newRosterName": " " }),
    ] {
        let out = tools::propose_roster_members(
            &ctx,
            rargs(json!({"target": target, "members": [{"name": "A"}]})),
        )
        .await
        .unwrap();
        assert!(!out.recorded);
        assert_eq!(out.errors.len(), 1);
        assert_eq!(out.errors[0].index, None);
    }
    assert!(ctx.snapshot().await.is_empty());
}

#[tokio::test]
async fn second_propose_for_same_target_replaces_first() {
    let e = env().await;
    let ctx = e.ctx(e.org);
    for name in ["王小明", "李小華"] {
        tools::propose_roster_members(
            &ctx,
            rargs(json!({"target": {"newRosterName": "業務部"}, "members": [{"name": name}]})),
        )
        .await
        .unwrap();
    }
    tools::propose_roster_members(
        &ctx,
        rargs(json!({"target": {"newRosterName": "行政"}, "members": [{"name": "Z"}]})),
    )
    .await
    .unwrap();
    let p = ctx.snapshot().await;
    assert_eq!(p.targets.len(), 2);
    let ProposalTarget::Roster(t) = &p.targets[0] else {
        panic!()
    };
    assert_eq!(t.members.len(), 1);
    assert_eq!(t.members[0].name, "李小華");
}

#[tokio::test]
async fn roster_limits() {
    let e = env().await;
    let ctx = e.ctx(e.org);
    let many = |n: usize, p: &str| -> Vec<serde_json::Value> {
        (0..n)
            .map(|i| json!({ "name": format!("{p}{i}") }))
            .collect()
    };
    let ok = tools::propose_roster_members(
        &ctx,
        rargs(json!({"target": {"newRosterName": "大"}, "members": many(500, "p")})),
    )
    .await
    .unwrap();
    assert!(ok.recorded && ok.created == 500);
    let over = tools::propose_roster_members(
        &ctx,
        rargs(json!({"target": {"newRosterName": "更大"}, "members": many(501, "p")})),
    )
    .await
    .unwrap();
    assert!(!over.recorded);
    assert!(over.errors[0].message.contains("500"));

    // existing + new > limit; updating existing ones at the limit is fine
    let members: Vec<_> = (0..500).map(|i| (format!("e{i}"), None::<&str>)).collect();
    let refs: Vec<(&str, Option<&str>)> = members.iter().map(|(n, x)| (n.as_str(), *x)).collect();
    let id = e.roster(e.org, "滿", &refs).await;
    let over = tools::propose_roster_members(&ctx, rargs(json!({"target": {"rosterId": id.to_string()}, "members": [{"name": "e0", "note": "x"}, {"name": "extra"}]})))
        .await
        .unwrap();
    assert!(!over.recorded);
    let ok = tools::propose_roster_members(&ctx, rargs(json!({"target": {"rosterId": id.to_string()}, "members": [{"name": "e0", "note": "x"}]})))
        .await
        .unwrap();
    assert_eq!(ok.updated, 1);
}

// ===========================================================================
// propose_vendor
// ===========================================================================

#[tokio::test]
async fn propose_vendor_new_and_existing_by_name() {
    let e = env().await;
    let ctx = e.ctx(e.org);
    let out = tools::propose_vendor(
        &ctx,
        vargs(json!({
            "target": "new",
            "info": { "name": "好吃便當", "phone": "02-1234", "address": null },
            "menuItems": [{ "name": "滷肉飯", "price": 60, "confidence": "high" }]
        })),
    )
    .await
    .unwrap();
    assert!(out.recorded && out.notes.is_empty());
    assert_eq!((out.created, out.info.as_deref()), (1, Some("create")));
    let p = ctx.snapshot().await;
    let ProposalTarget::Vendor(t) = &p.targets[0] else {
        panic!()
    };
    assert_eq!(
        t.vendor,
        TargetRef::New {
            name: "好吃便當".into()
        }
    );
    assert_eq!(t.info.before, None);
    assert_eq!(t.info.after.phone.as_deref(), Some("02-1234"));

    // now the vendor exists (different spelling) -> treated as existing
    let vid = e
        .vendor(
            e.org,
            "好吃便當",
            Some("02-1234"),
            &[("排骨飯", 100), ("素食便當", 90)],
        )
        .await;
    let ctx = e.ctx(e.org);
    let out = tools::propose_vendor(
        &ctx,
        vargs(json!({
            "target": "new",
            "info": { "name": "好吃　便當", "address": "台北市", "note": "滿 10 個外送" },
            "menuItems": [
                { "name": "排骨飯", "price": 105 },
                { "name": "素食便當", "price": 90 },
                { "name": "雞腿飯", "price": null, "confidence": "low" }
            ],
            "warnings": []
        })),
    )
    .await
    .unwrap();
    // ("好吃　便當" has a full-width space: same NameKey as "好吃 便當", not "好吃便當")
    assert!(out.notes.is_empty());
    let out = tools::propose_vendor(
        &ctx,
        vargs(json!({
            "target": "new",
            "info": { "name": " 好吃便當 ", "address": "台北市", "note": "滿 10 個外送" },
            "menuItems": [
                { "name": "排骨飯", "price": 105 },
                { "name": "素食便當", "price": 90 },
                { "name": "雞腿飯", "price": null, "confidence": "low" }
            ]
        })),
    )
    .await
    .unwrap();
    assert_eq!(out.notes.len(), 1);
    assert!(out.notes[0].contains("already exists"));
    assert_eq!((out.created, out.updated, out.unchanged), (1, 1, 1));
    assert_eq!(out.info.as_deref(), Some("update"));
    assert_eq!(out.price_changes.len(), 1);
    assert_eq!(
        (out.price_changes[0].from, out.price_changes[0].to),
        (100, 105)
    );
    assert!(
        out.errors.is_empty(),
        "null price is not an error: {:?}",
        out.errors
    );
    let v = serde_json::to_value(&out).unwrap();
    assert_eq!(v["priceChanges"][0]["name"], "排骨飯");

    let p = ctx.snapshot().await;
    // the second call replaced nothing for the first (different target kind)
    let ProposalTarget::Vendor(t) = p.targets.last().unwrap() else {
        panic!()
    };
    assert_eq!(t.vendor, TargetRef::Existing { existing_id: vid });
    assert_eq!(t.info.action, ChangeAction::Update);
    assert_eq!(t.info.before.as_ref().unwrap().address, None);
    assert_eq!(t.info.after.address.as_deref(), Some("台北市"));
    assert_eq!(t.info.after.phone.as_deref(), Some("02-1234")); // kept
    assert_eq!(t.menu[0].before.as_ref().unwrap().price, 100);
    assert_eq!(t.menu[0].price, Some(105));
    assert_eq!(t.menu[2].price, None);
    assert_eq!(t.menu[2].issues, [Issue::PriceMissing]);
    assert_eq!(t.menu[2].action, ChangeAction::Create);
}

#[tokio::test]
async fn propose_vendor_existing_target_is_not_renamed_and_price_errors() {
    let e = env().await;
    let vid = e.vendor(e.org, "好吃便當", None, &[("排骨飯", 100)]).await;
    let theirs = e.vendor(e.other, "別人", None, &[]).await;
    let ctx = e.ctx(e.org);
    let out = tools::propose_vendor(
        &ctx,
        vargs(json!({
            "target": { "vendorId": vid.to_string() },
            "info": { "name": "好吃便當總店" },
            "menuItems": [
                { "name": "A", "price": 0 },
                { "name": "B", "price": 100001 },
                { "name": "C", "price": 1 },
                { "name": "", "price": 5 },
                { "name": "c", "price": 5 }
            ]
        })),
    )
    .await
    .unwrap();
    assert!(out.recorded);
    assert!(out.notes[0].contains("is not renamed"));
    let errs: Vec<_> = out
        .errors
        .iter()
        .map(|x| (x.index, x.field.as_str()))
        .collect();
    assert_eq!(
        errs,
        [
            (Some(0), "price"),
            (Some(1), "price"),
            (Some(3), "name"),
            (Some(4), "name")
        ]
    );
    assert!(
        out.errors[0]
            .message
            .contains("price must be between 1 and 100000")
    );
    let p = ctx.snapshot().await;
    let ProposalTarget::Vendor(t) = &p.targets[0] else {
        panic!()
    };
    assert_eq!(t.info.after.name, "好吃便當");
    assert_eq!(t.menu[0].issues, [Issue::PriceOutOfRange]);
    assert_eq!(t.menu[0].price, Some(0));
    assert_eq!(t.menu[3].issues, [Issue::NameEmpty]);
    assert_eq!(t.menu[4].issues, [Issue::DuplicateInInput]);

    for target in [
        json!({"vendorId": theirs.to_string()}),
        json!({"vendorId": "x"}),
    ] {
        let out = tools::propose_vendor(
            &ctx,
            vargs(json!({"target": target, "info": {"name": "n"}, "menuItems": []})),
        )
        .await
        .unwrap();
        assert!(!out.recorded);
    }
    let out = tools::propose_vendor(
        &ctx,
        vargs(json!({"target": "new", "info": {"name": ""}, "menuItems": []})),
    )
    .await
    .unwrap();
    assert!(!out.recorded && out.errors[0].field == "info.name");
}

#[tokio::test]
async fn vendor_menu_limit_and_replace() {
    let e = env().await;
    let ctx = e.ctx(e.org);
    let items = |n: usize| -> Vec<serde_json::Value> {
        (0..n)
            .map(|i| json!({"name": format!("i{i}"), "price": 10}))
            .collect()
    };
    let out = tools::propose_vendor(
        &ctx,
        vargs(json!({"target": "new", "info": {"name": "店"}, "menuItems": items(201)})),
    )
    .await
    .unwrap();
    assert!(!out.recorded && out.errors[0].message.contains("200"));
    for n in [200, 3] {
        let out = tools::propose_vendor(
            &ctx,
            vargs(json!({"target": "new", "info": {"name": "店"}, "menuItems": items(n)})),
        )
        .await
        .unwrap();
        assert!(out.recorded);
    }
    let p = ctx.snapshot().await;
    assert_eq!(p.targets.len(), 1);
    let ProposalTarget::Vendor(t) = &p.targets[0] else {
        panic!()
    };
    assert_eq!(t.menu.len(), 3);
}

// ===========================================================================
// service: start
// ===========================================================================

fn roster_script(roster: &str, members: serde_json::Value) -> FakeImportAgent {
    FakeImportAgent::new(
        vec![
            FakeToolCall::ListRosters,
            FakeToolCall::ProposeRosterMembers(rargs(json!({
                "target": {"newRosterName": roster}, "members": members, "warnings": ["看一下"]
            }))),
        ],
        "辨識出 2 位成員",
    )
}

#[tokio::test]
async fn start_with_fake_agent_ends_proposed_and_is_persisted() {
    let e = env().await;
    let svc = e.service(
        Arc::new(roster_script(
            "業務部",
            json!([{"name": "A"}, {"name": "B", "note": "x"}]),
        )),
        10,
    );
    let run = svc.start(e.org, cmd()).await.unwrap();
    assert_eq!(run.status, ImportRunStatus::Proposed);
    assert_eq!(run.summary.as_deref(), Some("辨識出 2 位成員"));
    assert_eq!(run.model, "fake");
    assert_eq!(
        run.usage,
        Usage {
            input_tokens: 1000,
            output_tokens: 200
        }
    );
    assert_eq!(run.proposal.targets.len(), 1);
    assert_eq!(run.proposal.warnings, ["看一下"]);

    let got = svc.get(e.org, run.id).await.unwrap();
    assert_eq!(got.status, ImportRunStatus::Proposed);
    assert_eq!(got.proposal, run.proposal);
    assert_eq!(
        (got.summary, got.model, got.usage),
        (run.summary, run.model, run.usage)
    );
    assert!(got.finished_at.is_some());
    assert!(matches!(
        svc.get(e.other, run.id).await,
        Err(AiImportError::NotFound)
    ));
}

#[tokio::test]
async fn start_validates_targets_and_quota() {
    let e = env().await;
    let theirs = e.roster(e.other, "x", &[]).await;
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 2);
    let mut c = cmd();
    c.target_roster_id = Some(theirs);
    assert!(matches!(
        svc.start(e.org, c).await,
        Err(AiImportError::NotFound)
    ));
    let mut c = cmd();
    c.target_vendor_id = Some(VendorId::new());
    assert!(matches!(
        svc.start(e.org, c).await,
        Err(AiImportError::NotFound)
    ));
    assert!(e.run_status(e.org).await.is_empty());

    // empty proposal is still a Proposed run
    let run = svc.start(e.org, cmd()).await.unwrap();
    assert!(run.proposal.is_empty());
    svc.start(e.org, cmd()).await.unwrap();
    assert!(matches!(
        svc.start(e.org, cmd()).await,
        Err(AiImportError::QuotaExceeded)
    ));
    // quota is per organizer
    svc.start(e.other, cmd()).await.unwrap();
    // only runs of the last 24 h count
    sqlx::query(
        "UPDATE ai_import_runs SET created_at = '2020-01-01T00:00:00Z' WHERE organizer_id = ?",
    )
    .bind(e.org.to_string())
    .execute(&e.db.write)
    .await
    .unwrap();
    svc.start(e.org, cmd()).await.unwrap();
}

/// Fails with scripted errors, then succeeds; counts calls.
struct Flaky {
    calls: AtomicUsize,
    failures: Vec<AiError>,
}

#[async_trait]
impl ImportAgent for Flaky {
    async fn run(
        &self,
        _ctx: Arc<ImportToolContext>,
        _i: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        match self.failures.get(n) {
            Some(e) => Err(e.clone()),
            None => Ok(AgentOutcome {
                summary: "ok".into(),
                usage: Usage::default(),
                model: "m".into(),
            }),
        }
    }
}

fn unavailable(retryable: bool) -> AiError {
    AiError::Unavailable {
        retryable,
        detail: "529".into(),
    }
}

#[tokio::test]
async fn retryable_failure_is_retried_once() {
    let e = env().await;
    // fails once -> second attempt succeeds
    let agent = Arc::new(Flaky {
        calls: AtomicUsize::new(0),
        failures: vec![unavailable(true)],
    });
    let svc = e.service(agent.clone(), 10);
    let run = svc.start(e.org, cmd()).await.unwrap();
    assert_eq!(run.status, ImportRunStatus::Proposed);
    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);

    // fails twice -> one retry only, run marked failed
    let agent = Arc::new(Flaky {
        calls: AtomicUsize::new(0),
        failures: vec![unavailable(true); 5],
    });
    let svc = e.service(agent.clone(), 10);
    let err = svc.start(e.org, cmd()).await.unwrap_err();
    assert!(matches!(
        err,
        AiImportError::Agent {
            error: AiError::Unavailable {
                retryable: true,
                ..
            },
            ..
        }
    ));
    assert_eq!(agent.calls.load(Ordering::SeqCst), 2);
    assert_eq!(e.run_status(e.org).await, ["proposed", "failed"]);

    // not retryable -> a single call
    let agent = Arc::new(Flaky {
        calls: AtomicUsize::new(0),
        failures: vec![unavailable(false)],
    });
    let svc = e.service(agent.clone(), 10);
    assert!(svc.start(e.org, cmd()).await.is_err());
    assert_eq!(agent.calls.load(Ordering::SeqCst), 1);

    // refusal is mapped through and recorded
    let agent = Arc::new(Flaky {
        calls: AtomicUsize::new(0),
        failures: vec![AiError::Refused {
            category: Some("x".into()),
        }],
    });
    let svc = e.service(agent, 10);
    assert!(matches!(
        svc.start(e.org, cmd()).await,
        Err(AiImportError::Agent {
            error: AiError::Refused { .. },
            ..
        })
    ));
    let (err,): (Option<String>,) = sqlx::query_as(
        "SELECT error FROM ai_import_runs WHERE status = 'failed' ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .fetch_one(&e.db.read)
    .await
    .unwrap();
    assert!(err.unwrap().contains("refused"));
}

struct Sleeper;

#[async_trait]
impl ImportAgent for Sleeper {
    async fn run(
        &self,
        _ctx: Arc<ImportToolContext>,
        _i: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        tokio::time::sleep(Duration::from_secs(30)).await;
        unreachable!()
    }
}

#[tokio::test]
async fn agent_timeout_fails_the_run() {
    let e = env().await;
    let svc = e.service(Arc::new(Sleeper), 10);
    let mut command = cmd();
    command.mode = ImportMode::Vendor;
    command.pending_photo = Some(pending_photo());
    let err = svc.start(e.org, command).await.unwrap_err();
    assert!(matches!(
        err,
        AiImportError::Agent {
            error: AiError::Timeout,
            ..
        }
    ));
    assert_eq!(e.run_status(e.org).await, ["failed"]);
    assert_eq!(photo_count(&e).await, 0);
}

// ===========================================================================
// service: apply / discard
// ===========================================================================

#[tokio::test]
async fn shared_import_is_actor_and_scope_bound_and_rechecks_admin_on_apply() {
    use application::ports::{ImportApplyRepo, ImportApplyStep};
    use application::sharing::SharingRepo;
    let e = env().await;
    let groups = crate::sharing_repo::SqliteSharingRepo::new(e.db.clone());
    let g = groups.create(e.org, "Team").await.unwrap();
    let g2 = groups.create(e.org, "Other").await.unwrap();
    groups.add(e.org, g.id, e.other, "admin").await.unwrap();
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 100);
    let mut command = cmd();
    command.sharing_group_id = Some(g.id);
    let run = svc.start(e.org, command.clone()).await.unwrap();
    assert_eq!(
        svc.get(e.org, run.id).await.unwrap().sharing_group_id,
        Some(g.id)
    );
    assert!(matches!(
        svc.get(e.other, run.id).await,
        Err(AiImportError::NotFound)
    ));
    assert!(matches!(
        svc.apply(e.other, run.id, ApplyCmd::default()).await,
        Err(AiImportError::NotFound)
    ));
    let private = e.roster(e.org, "Private", &[("A", None)]).await;
    let mut other = Roster::new(e.org, "Other").unwrap();
    other.sharing_group_id = Some(g2.id);
    let other = e
        .rosters
        .create(other, SystemTime::now())
        .await
        .unwrap()
        .value
        .id();
    for target in [private, other] {
        let mut bad = command.clone();
        bad.target_roster_id = Some(target);
        assert!(matches!(
            svc.start(e.org, bad).await,
            Err(AiImportError::NotFound)
        ));
        assert!(matches!(
            svc.apply(
                e.org,
                run.id,
                ApplyCmd {
                    save_photo: None,
                    accepted: vec![acc_roster(
                        TargetRef::Existing {
                            existing_id: target
                        },
                        &[("Injected", None)]
                    )]
                }
            )
            .await,
            Err(AiImportError::InvalidChanges(_))
        ));
    }
    // Successful creates inherit the draft scope, never a caller-selected scope.
    let applied = svc
        .apply(
            e.org,
            run.id,
            ApplyCmd {
                save_photo: None,
                accepted: vec![
                    acc_roster(
                        TargetRef::New {
                            name: "Imported".into(),
                        },
                        &[("Alice", None)],
                    ),
                    acc_vendor(
                        TargetRef::New {
                            name: "Cafe".into(),
                        },
                        info("Cafe", None, None),
                        &[("Rice", Some(80))],
                    ),
                ],
            },
        )
        .await
        .unwrap();
    let rid = applied.roster_ids[0];
    assert_eq!(
        e.rosters
            .find(e.other, rid)
            .await
            .unwrap()
            .unwrap()
            .value
            .sharing_group_id,
        Some(g.id)
    );
    let vid = applied.vendor_ids[0];
    assert_eq!(
        e.vendors
            .find(e.other, vid)
            .await
            .unwrap()
            .unwrap()
            .value
            .sharing_group_id,
        Some(g.id)
    );
    let mut existing = command.clone();
    existing.target_roster_id = Some(rid);
    let pending = svc.start(e.org, existing.clone()).await.unwrap();
    groups
        .change(e.other, g.id, e.org, Some("member"))
        .await
        .unwrap();
    assert!(
        svc.get(e.org, pending.id).await.is_ok(),
        "member may read own draft"
    );
    assert!(matches!(
        svc.start(e.org, existing).await,
        Err(AiImportError::NotFound)
    ));
    assert!(matches!(
        svc.apply(e.org, pending.id, ApplyCmd::default()).await,
        Err(AiImportError::NotFound)
    ));
    let mut in_memory = pending.clone();
    in_memory.apply(SystemTime::now()).unwrap();
    let mut bypass = Roster::new(e.org, "Bypass").unwrap();
    bypass.sharing_group_id = Some(g.id);
    assert!(
        e.runs
            .apply(
                &in_memory,
                vec![ImportApplyStep::CreateRoster(bypass)],
                None
            )
            .await
            .is_err(),
        "transaction rechecks even an already-prepared apply"
    );
    groups.change(e.other, g.id, e.org, None).await.unwrap();
    assert!(matches!(
        svc.get(e.org, pending.id).await,
        Err(AiImportError::NotFound)
    ));
    assert!(matches!(
        svc.apply(e.org, pending.id, ApplyCmd::default()).await,
        Err(AiImportError::NotFound)
    ));
}

#[tokio::test]
async fn import_tools_cannot_escape_scope_and_recheck_revocation() {
    use application::sharing::SharingRepo;
    let e = env().await;
    let groups = crate::sharing_repo::SqliteSharingRepo::new(e.db.clone());
    let g = groups.create(e.org, "Team").await.unwrap();
    groups.add(e.org, g.id, e.other, "admin").await.unwrap();
    let private = e.roster(e.org, "Private", &[("Secret", None)]).await;
    let private_vendor = e.vendor(e.org, "Private", None, &[("Rice", 80)]).await;
    let mut r = Roster::new(e.other, "Shared").unwrap();
    r.sharing_group_id = Some(g.id);
    let shared = e
        .rosters
        .create(r, SystemTime::now())
        .await
        .unwrap()
        .value
        .id();
    let mut ctx = e.ctx(e.org);
    ctx.sharing_group_id = Some(g.id);
    assert_eq!(tools::list_rosters(&ctx).await.unwrap().len(), 1);
    assert!(tools::list_vendors(&ctx).await.unwrap().is_empty());
    assert!(matches!(
        tools::get_roster(
            &ctx,
            GetRosterArgs {
                roster_id: private.to_string()
            }
        )
        .await
        .unwrap(),
        GetRosterOutput::Error(_)
    ));
    assert!(matches!(
        tools::get_vendor(
            &ctx,
            GetVendorArgs {
                vendor_id: private_vendor.to_string()
            }
        )
        .await
        .unwrap(),
        GetVendorOutput::Error(_)
    ));
    assert!(
        !tools::propose_roster_members(
            &ctx,
            rargs(json!({"target":{"rosterId":private.to_string()},"members":[{"name":"No"}]}))
        )
        .await
        .unwrap()
        .recorded
    );
    assert!(!tools::propose_vendor(&ctx,vargs(json!({"target":{"vendorId":private_vendor.to_string()},"info":{"name":"No"},"menuItems":[]}))).await.unwrap().recorded);
    assert!(
        tools::propose_roster_members(
            &ctx,
            rargs(json!({"target":{"rosterId":shared.to_string()},"members":[{"name":"Yes"}]}))
        )
        .await
        .unwrap()
        .recorded
    );
    groups.change(e.other, g.id, e.org, None).await.unwrap();
    assert!(tools::list_rosters(&ctx).await.is_err());
    assert!(tools::list_vendors(&ctx).await.is_err());
    assert!(
        tools::get_roster(
            &ctx,
            GetRosterArgs {
                roster_id: shared.to_string()
            }
        )
        .await
        .is_err()
    );
    assert!(
        tools::propose_vendor(
            &ctx,
            vargs(json!({"target":"new","info":{"name":"No"},"menuItems":[]}))
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn shared_vendor_import_targets_and_same_name_matching_stay_in_draft_scope() {
    use application::sharing::SharingRepo;
    let e = env().await;
    let groups = crate::sharing_repo::SqliteSharingRepo::new(e.db.clone());
    let first = groups.create(e.org, "First").await.unwrap();
    let second = groups.create(e.org, "Second").await.unwrap();
    let empty = groups.create(e.org, "Empty").await.unwrap();
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 100);
    let scopes = [None, Some(first.id), Some(second.id)];
    let mut vendors = Vec::new();
    for scope in scopes {
        let mut vendor = Vendor::new(e.org, "Cafe").unwrap();
        vendor.sharing_group_id = scope;
        vendor
            .upsert_menu_items(vec![MenuItemInput {
                name: "Rice".into(),
                price: 80,
                description: None,
                category: None,
            }])
            .unwrap();
        vendors.push(
            e.vendors
                .create(vendor, SystemTime::now())
                .await
                .unwrap()
                .value,
        );
    }
    for (i, scope) in scopes.into_iter().enumerate() {
        let mut command = cmd();
        command.mode = ImportMode::Vendor;
        command.sharing_group_id = scope;
        command.target_vendor_id = Some(vendors[i].id());
        let run = svc.start(e.org, command.clone()).await.unwrap();
        for (j, vendor) in vendors.iter().enumerate() {
            if i == j {
                continue;
            }
            let mut bad_start = command.clone();
            bad_start.target_vendor_id = Some(vendor.id());
            assert!(matches!(
                svc.start(e.org, bad_start).await,
                Err(AiImportError::NotFound)
            ));
            assert!(matches!(
                svc.apply(
                    e.org,
                    run.id,
                    ApplyCmd {
                        save_photo: None,
                        accepted: vec![acc_vendor(
                            TargetRef::Existing {
                                existing_id: vendor.id()
                            },
                            info("Cafe", Some("Injected"), None),
                            &[("Rice", Some(999))],
                        )],
                    }
                )
                .await,
                Err(AiImportError::InvalidChanges(_))
            ));
            assert_eq!(
                svc.get(e.org, run.id).await.unwrap().status,
                ImportRunStatus::Proposed
            );
            assert_eq!(
                e.vendors
                    .find(e.org, vendor.id())
                    .await
                    .unwrap()
                    .unwrap()
                    .value,
                *vendor
            );
        }
        // A "new" target with the same normalized name must resolve only in
        // this draft's namespace, even when all three namespaces contain it.
        let price = 100 + i as i64;
        let result = svc
            .apply(
                e.org,
                run.id,
                ApplyCmd {
                    save_photo: None,
                    accepted: vec![acc_vendor(
                        TargetRef::New {
                            name: " cafe ".into(),
                        },
                        info(" cafe ", None, None),
                        &[("Rice", Some(price))],
                    )],
                },
            )
            .await
            .unwrap();
        assert_eq!(result.vendor_ids, [vendors[i].id()]);
        vendors[i] = e
            .vendors
            .find(e.org, vendors[i].id())
            .await
            .unwrap()
            .unwrap()
            .value;
        assert_eq!(vendors[i].sharing_group_id, scope);
        assert_eq!(vendors[i].menu()[0].price.get(), price);
        for vendor in &vendors {
            assert_eq!(
                e.vendors
                    .find(e.org, vendor.id())
                    .await
                    .unwrap()
                    .unwrap()
                    .value,
                *vendor
            );
        }
    }
    // No match in a fourth scope must create there, never fall back to a
    // same-name private or other-group vendor.
    let mut command = cmd();
    command.mode = ImportMode::Vendor;
    command.sharing_group_id = Some(empty.id);
    let run = svc.start(e.org, command).await.unwrap();
    let result = svc
        .apply(
            e.org,
            run.id,
            ApplyCmd {
                save_photo: None,
                accepted: vec![acc_vendor(
                    TargetRef::New {
                        name: "Cafe".into(),
                    },
                    info("Cafe", None, None),
                    &[("Rice", Some(200))],
                )],
            },
        )
        .await
        .unwrap();
    assert_eq!(result.vendor_ids.len(), 1);
    assert!(
        vendors
            .iter()
            .all(|vendor| vendor.id() != result.vendor_ids[0])
    );
    let created = e
        .vendors
        .find(e.org, result.vendor_ids[0])
        .await
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(created.sharing_group_id, Some(empty.id));
    assert_eq!(created.menu()[0].price.get(), 200);
    for vendor in &vendors {
        assert_eq!(
            e.vendors
                .find(e.org, vendor.id())
                .await
                .unwrap()
                .unwrap()
                .value,
            *vendor
        );
    }
}

#[tokio::test]
async fn shared_draft_cannot_create_private_address_or_touch_another_scope_in_transaction() {
    use application::ports::{ImportApplyRepo, ImportApplyStep};
    use application::sharing::SharingRepo;
    let e = env().await;
    let groups = crate::sharing_repo::SqliteSharingRepo::new(e.db.clone());
    let g = groups.create(e.org, "Team").await.unwrap();
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 100);
    let mut c = cmd();
    c.sharing_group_id = Some(g.id);
    c.mode = ImportMode::DeliveryAddress;
    assert!(svc.start(e.org, c.clone()).await.is_err());
    c.mode = ImportMode::Roster;
    let run = svc.start(e.org, c).await.unwrap();
    assert!(
        svc.apply(
            e.org,
            run.id,
            ApplyCmd {
                save_photo: None,
                accepted: vec![AcceptedChange::DeliveryAddress(
                    application::ai_import::AcceptedDeliveryAddress {
                        name: "Home".into(),
                        address: "Address".into()
                    }
                )]
            }
        )
        .await
        .is_err()
    );
    let private = e.roster(e.org, "Private", &[("A", None)]).await;
    let mut applied = run.clone();
    applied.apply(SystemTime::now()).unwrap();
    let address = domain::directory::DeliveryAddress::new(e.org, "Home", "Address").unwrap();
    let mut preceding = Roster::new(e.org, "Must roll back").unwrap();
    preceding.sharing_group_id = Some(g.id);
    let preceding_id = preceding.id();
    assert!(matches!(
        e.runs
            .apply(
                &applied,
                vec![
                    ImportApplyStep::CreateRoster(preceding),
                    ImportApplyStep::CreateDeliveryAddress(address),
                ],
                None
            )
            .await,
        Err(application::ports::RepoError::NotFound)
    ));
    assert_eq!(
        svc.get(e.org, run.id).await.unwrap().status,
        ImportRunStatus::Proposed
    );
    assert!(e.rosters.find(e.org, preceding_id).await.unwrap().is_none());
    let (address_count,): (i64,) = sqlx::query_as("SELECT count(*) FROM delivery_addresses")
        .fetch_one(&e.db.read)
        .await
        .unwrap();
    assert_eq!(address_count, 0);
    assert!(
        e.runs
            .apply(
                &applied,
                vec![ImportApplyStep::ModifyRoster {
                    id: private,
                    mutation: Box::new(|r| {
                        r.rename("Hacked")?;
                        Ok(None)
                    })
                }],
                None
            )
            .await
            .is_err()
    );
    assert_eq!(
        svc.get(e.org, run.id).await.unwrap().status,
        ImportRunStatus::Proposed
    );
    assert_eq!(
        e.rosters
            .find(e.org, private)
            .await
            .unwrap()
            .unwrap()
            .value
            .name()
            .as_str(),
        "Private"
    );
}

fn acc_roster(target: TargetRef<RosterId>, members: &[(&str, Option<&str>)]) -> AcceptedChange {
    AcceptedChange::Roster(AcceptedRoster {
        roster: target,
        members: members
            .iter()
            .map(|(n, note)| AcceptedMember {
                name: (*n).into(),
                note: note.map(Into::into),
            })
            .collect(),
    })
}

fn acc_vendor(
    target: TargetRef<VendorId>,
    info: Option<AcceptedVendorInfo>,
    menu: &[(&str, Option<i64>)],
) -> AcceptedChange {
    AcceptedChange::Vendor(AcceptedVendor {
        vendor: target,
        info,
        menu: menu
            .iter()
            .map(|(n, p)| AcceptedMenuItem {
                name: (*n).into(),
                price: *p,
                description: None,
                category: None,
            })
            .collect(),
    })
}

fn info(name: &str, phone: Option<&str>, address: Option<&str>) -> Option<AcceptedVendorInfo> {
    Some(AcceptedVendorInfo {
        name: Some(name.into()),
        phone: phone.map(Into::into),
        address: address.map(Into::into),
        note: None,
    })
}

fn pending_photo() -> application::ports::NormalizedVendorImage {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(3, 2)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    crate::vendor_image::normalize(bytes.get_ref()).unwrap()
}

async fn photo_run(e: &Env) -> domain::ai_import::ImportRun {
    use application::ports::ImportRunRepo;
    let mut run = domain::ai_import::ImportRun::start(
        e.org,
        ImportMode::Vendor,
        None,
        None,
        None,
        "m".into(),
        SystemTime::now(),
    );
    e.runs.create(&run, Some(pending_photo())).await.unwrap();
    run.propose(
        Default::default(),
        "s".into(),
        Usage::default(),
        SystemTime::now(),
    )
    .unwrap();
    e.runs.save(&run, ImportRunStatus::Running).await.unwrap();
    e.runs.find(e.org, run.id).await.unwrap().unwrap()
}

async fn photo_count(e: &Env) -> i64 {
    sqlx::query_as::<_, (i64,)>("SELECT count(*) FROM ai_import_photos")
        .fetch_one(&e.db.read)
        .await
        .unwrap()
        .0
}

struct PhotoAgent(Vec<u8>);
#[async_trait]
impl ImportAgent for PhotoAgent {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        input: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        assert_eq!(input.image.bytes, self.0);
        assert_eq!(input.image.media_type, "image/jpeg");
        tools::propose_vendor(&ctx, vargs(json!({"target":"new","info":{"name":"Photo shop"},"menuItems":[{"name":"Rice","price":60,"confidence":"high"}]}))).await.unwrap();
        Ok(AgentOutcome {
            summary: "s".into(),
            model: "m".into(),
            usage: Usage::default(),
        })
    }
}

#[tokio::test]
async fn pending_photo_service_normalized_input_unique_target_scope_and_clock() {
    use application::ports::ImportRunRepo;
    use application::sharing::SharingRepo;
    struct Fixed(SystemTime);
    impl application::ports::Clock for Fixed {
        fn now(&self) -> SystemTime {
            self.0
        }
    }
    let e = env().await;
    let groups = crate::sharing_repo::SqliteSharingRepo::new(e.db.clone());
    let group = groups.create(e.org, "Photos").await.unwrap();
    let mut svc = e.service(Arc::new(PhotoAgent(pending_photo().data)), 100);
    let mut command = cmd();
    command.mode = ImportMode::Vendor;
    command.sharing_group_id = Some(group.id);
    command.pending_photo = Some(pending_photo());
    let accepted = || {
        vec![acc_vendor(
            TargetRef::New {
                name: "Photo shop".into(),
            },
            info("Photo shop", None, None),
            &[("Rice", Some(60))],
        )]
    };
    let run = svc.start(e.org, command.clone()).await.unwrap();
    let expiry = run.pending_photo_expires_at.unwrap();
    svc.clock = Arc::new(Fixed(expiry));
    assert!(
        svc.get(e.org, run.id)
            .await
            .unwrap()
            .pending_photo_expires_at
            .is_none()
    );
    assert!(
        svc.apply(
            e.org,
            run.id,
            ApplyCmd {
                accepted: accepted(),
                save_photo: Some(true)
            }
        )
        .await
        .is_err()
    );
    svc.clock = Arc::new(SystemClock);
    let mut multiple = run.clone();
    multiple
        .proposal
        .targets
        .push(multiple.proposal.targets[0].clone());
    e.runs
        .save(&multiple, ImportRunStatus::Proposed)
        .await
        .unwrap();
    assert!(
        svc.apply(
            e.org,
            run.id,
            ApplyCmd {
                accepted: accepted(),
                save_photo: Some(true)
            }
        )
        .await
        .is_err()
    );
    assert!(
        svc.apply(
            e.org,
            run.id,
            ApplyCmd {
                accepted: vec![],
                save_photo: Some(true)
            }
        )
        .await
        .is_err()
    );
    e.runs.save(&run, ImportRunStatus::Proposed).await.unwrap();
    let result = svc
        .apply(
            e.org,
            run.id,
            ApplyCmd {
                accepted: accepted(),
                save_photo: None,
            },
        )
        .await
        .unwrap();
    let id = result.vendor_ids[0];
    let first = e
        .vendors
        .find(e.org, id)
        .await
        .unwrap()
        .unwrap()
        .value
        .image
        .unwrap();
    assert_eq!(photo_count(&e).await, 0);
    // Scoped same-name resolution replaces exactly that vendor's photo.
    let run = svc.start(e.org, command.clone()).await.unwrap();
    let result = svc
        .apply(
            e.org,
            run.id,
            ApplyCmd {
                accepted: accepted(),
                save_photo: Some(true),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.vendor_ids, [id]);
    assert_ne!(
        e.vendors
            .find(e.org, id)
            .await
            .unwrap()
            .unwrap()
            .value
            .image
            .unwrap()
            .revision,
        first.revision
    );
    // Revocation after proposal prevents any photo/menu/state mutation.
    let run = svc.start(e.org, command).await.unwrap();
    groups.add(e.org, group.id, e.other, "admin").await.unwrap();
    groups.change(e.other, group.id, e.org, None).await.unwrap();
    assert!(matches!(
        svc.apply(
            e.org,
            run.id,
            ApplyCmd {
                accepted: accepted(),
                save_photo: Some(true)
            }
        )
        .await,
        Err(AiImportError::NotFound)
    ));
    assert_eq!(photo_count(&e).await, 1);
    let stored = e.runs.find(e.org, run.id).await.unwrap().unwrap();
    assert_eq!(stored.status, ImportRunStatus::Proposed);
    // Another connection sees the durable proposal and metadata, without BLOB loading.
    let reopened = Db::connect_path(&e._dir.path().join("test.db"))
        .await
        .unwrap();
    assert_eq!(
        SqliteImportRunRepo::new(reopened)
            .find(e.org, run.id)
            .await
            .unwrap()
            .unwrap(),
        stored
    );
}

#[tokio::test]
async fn pending_photo_atomic_rollback_replace_and_double_cas() {
    use application::ports::{ImportApplyRepo, ImportApplyStep, ImportRunRepo};
    let e = env().await;
    let id = e.vendor(e.org, "Existing", None, &[("Old", 50)]).await;
    let old = e
        .vendors
        .put_image(e.org, id, pending_photo(), SystemTime::now())
        .await
        .unwrap();
    let mut run = photo_run(&e).await;
    run.apply(SystemTime::now()).unwrap();
    sqlx::query("CREATE TRIGGER reject_photo BEFORE UPDATE ON vendor_images BEGIN SELECT RAISE(ABORT, 'test'); END").execute(&e.db.write).await.unwrap();
    let steps = || {
        vec![ImportApplyStep::ModifyVendor {
            id,
            mutation: Box::new(|v| {
                v.upsert_menu_items(vec![MenuItemInput {
                    name: "New".into(),
                    price: 80,
                    description: None,
                    category: None,
                }])
                .map(Some)
            }),
        }]
    };
    assert!(e.runs.apply(&run, steps(), Some(id)).await.is_err());
    assert_eq!(
        e.runs.find(e.org, run.id).await.unwrap().unwrap().status,
        ImportRunStatus::Proposed
    );
    assert_eq!(photo_count(&e).await, 1);
    let vendor = e.vendors.find(e.org, id).await.unwrap().unwrap().value;
    assert_eq!(vendor.menu().len(), 1);
    assert_eq!(vendor.image.as_ref().unwrap().revision, old.revision);
    sqlx::query("DROP TRIGGER reject_photo")
        .execute(&e.db.write)
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        e.runs.apply(&run, steps(), Some(id)),
        e.runs.apply(&run, steps(), Some(id))
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(photo_count(&e).await, 0);
    let vendor = e.vendors.find(e.org, id).await.unwrap().unwrap().value;
    assert_eq!(vendor.menu().len(), 2);
    assert_ne!(vendor.image.as_ref().unwrap().revision, old.revision);
}

#[tokio::test]
async fn pending_photo_expiry_rolls_back_create_but_opt_out_succeeds() {
    use application::ports::{ImportApplyRepo, ImportApplyStep, ImportRunRepo};
    let e = env().await;
    let mut run = photo_run(&e).await;
    let expiry = run.created_at + Duration::from_secs(86400);
    assert_eq!(
        run.pending_photo_expires_at,
        Some(crate::timefmt::from_text(&crate::timefmt::to_text(expiry)).unwrap())
    );
    run.apply(expiry).unwrap();
    let vendor = Vendor::new(e.org, "New vendor").unwrap();
    let id = vendor.id();
    assert!(matches!(
        e.runs
            .apply(
                &run,
                vec![ImportApplyStep::CreateVendor(vendor.clone())],
                Some(id)
            )
            .await,
        Err(application::ports::RepoError::Domain(
            domain::shared::DomainError::Invalid {
                field: "savePhoto",
                ..
            }
        ))
    ));
    assert!(e.vendors.find(e.org, id).await.unwrap().is_none());
    assert_eq!(
        e.runs.find(e.org, run.id).await.unwrap().unwrap().status,
        ImportRunStatus::Proposed
    );
    assert_eq!(photo_count(&e).await, 1);
    e.runs
        .apply(&run, vec![ImportApplyStep::CreateVendor(vendor)], None)
        .await
        .unwrap();
    assert_eq!(photo_count(&e).await, 0);
    assert!(
        e.vendors
            .find(e.org, id)
            .await
            .unwrap()
            .unwrap()
            .value
            .image
            .is_none()
    );
}

#[tokio::test]
async fn pending_photo_discard_failed_cas_and_maintenance_cleanup() {
    use application::MaintenanceRepo;
    use application::ports::ImportRunRepo;
    let e = env().await;
    let mut discard = photo_run(&e).await;
    discard.discard().unwrap();
    // A failed CAS must not clear a still-retryable photo.
    assert!(
        e.runs
            .save(&discard, ImportRunStatus::Running)
            .await
            .is_err()
    );
    assert_eq!(photo_count(&e).await, 1);
    e.runs
        .save(&discard, ImportRunStatus::Proposed)
        .await
        .unwrap();
    assert_eq!(photo_count(&e).await, 0);
    let mut failed = domain::ai_import::ImportRun::start(
        e.org,
        ImportMode::Vendor,
        None,
        None,
        None,
        "m".into(),
        SystemTime::now(),
    );
    e.runs.create(&failed, Some(pending_photo())).await.unwrap();
    failed
        .fail("test".into(), Usage::default(), SystemTime::now())
        .unwrap();
    e.runs
        .save(&failed, ImportRunStatus::Running)
        .await
        .unwrap();
    assert_eq!(photo_count(&e).await, 0);
    let live = photo_run(&e).await;
    let expired = photo_run(&e).await;
    sqlx::query("UPDATE ai_import_photos SET expires_at=? WHERE run_id=?")
        .bind(crate::timefmt::to_text(SystemTime::UNIX_EPOCH))
        .bind(expired.id.to_string())
        .execute(&e.db.write)
        .await
        .unwrap();
    let stuck = domain::ai_import::ImportRun::start(
        e.org,
        ImportMode::Vendor,
        None,
        None,
        None,
        "m".into(),
        SystemTime::UNIX_EPOCH,
    );
    e.runs.create(&stuck, Some(pending_photo())).await.unwrap();
    let terminal = photo_run(&e).await;
    sqlx::query("UPDATE ai_import_runs SET status='discarded' WHERE id=?")
        .bind(terminal.id.to_string())
        .execute(&e.db.write)
        .await
        .unwrap();
    let vendor_id = e.vendor(e.org, "Retained", None, &[]).await;
    let saved = e
        .vendors
        .put_image(e.org, vendor_id, pending_photo(), SystemTime::now())
        .await
        .unwrap();
    let maintenance = crate::maintenance_repo::SqliteMaintenanceRepo::new(e.db.clone());
    assert_eq!(
        maintenance
            .fail_stuck_import_runs(
                SystemTime::now() - Duration::from_secs(600),
                SystemTime::now()
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(photo_count(&e).await, 1);
    assert_eq!(
        e.vendors
            .find(e.org, vendor_id)
            .await
            .unwrap()
            .unwrap()
            .value
            .image
            .unwrap(),
        saved
    );
    assert!(
        e.runs
            .find(e.org, live.id)
            .await
            .unwrap()
            .unwrap()
            .pending_photo_expires_at
            .is_some()
    );
    sqlx::query("DELETE FROM ai_import_runs WHERE id=?")
        .bind(live.id.to_string())
        .execute(&e.db.write)
        .await
        .unwrap();
    assert_eq!(photo_count(&e).await, 0);
}

#[tokio::test]
async fn pending_photo_apply_discard_race_has_one_winner() {
    use application::ports::{ImportApplyRepo, ImportApplyStep, ImportRunRepo};
    let e = env().await;
    let run = photo_run(&e).await;
    let mut applied = run.clone();
    applied.apply(SystemTime::now()).unwrap();
    let mut discarded = run;
    discarded.discard().unwrap();
    let vendor = Vendor::new(e.org, "Race").unwrap();
    let id = vendor.id();
    let (apply, discard) = tokio::join!(
        e.runs.apply(
            &applied,
            vec![ImportApplyStep::CreateVendor(vendor)],
            Some(id)
        ),
        e.runs.save(&discarded, ImportRunStatus::Proposed)
    );
    assert_ne!(apply.is_ok(), discard.is_ok());
    assert_eq!(photo_count(&e).await, 0);
    let vendor = e.vendors.find(e.org, id).await.unwrap();
    assert_eq!(vendor.is_some(), apply.is_ok());
    if let Some(vendor) = vendor {
        assert!(vendor.value.image.is_some());
    }
}

async fn proposed_run(e: &Env, svc: &AiImportService) -> domain::ai_import::ImportRunId {
    let _ = e;
    svc.start(e.org, cmd()).await.unwrap().id
}

#[tokio::test]
async fn apply_roster_writes_only_the_accepted_subset() {
    let e = env().await;
    let id = e
        .roster(e.org, "業務", &[("Amy", Some("素食")), ("Bob", None)])
        .await;
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 10);
    let run = proposed_run(&e, &svc).await;
    let res = svc
        .apply(
            e.org,
            run,
            ApplyCmd {
                save_photo: None,
                accepted: vec![acc_roster(
                    TargetRef::Existing { existing_id: id },
                    // proposal might have had more; the user accepted these (one edited)
                    &[
                        ("ＡＭＹ", Some("  ")),
                        ("Bob", Some("改過的備註")),
                        ("新人（編輯後）", Some("n")),
                    ],
                )],
            },
        )
        .await
        .unwrap();
    assert_eq!(res.roster_ids, [id]);
    assert!(res.vendor_ids.is_empty());
    let r = e.rosters.find(e.org, id).await.unwrap().unwrap().value;
    let names: Vec<_> = r
        .members()
        .iter()
        .map(|m| (m.name.as_str(), m.note.as_deref()))
        .collect();
    assert_eq!(
        names,
        [
            ("Amy", Some("素食")),
            ("Bob", Some("改過的備註")),
            ("新人（編輯後）", Some("n"))
        ]
    );
    let run = svc.get(e.org, run).await.unwrap();
    assert_eq!(run.status, ImportRunStatus::Applied);
    assert!(run.applied_at.is_some());

    // applying twice fails on status and changes nothing
    let again = svc
        .apply(
            e.org,
            run.id,
            ApplyCmd {
                save_photo: None,
                accepted: vec![acc_roster(
                    TargetRef::Existing { existing_id: id },
                    &[("Zed", None)],
                )],
            },
        )
        .await;
    assert!(matches!(again, Err(AiImportError::InvalidState(_))));
    assert_eq!(
        e.rosters
            .find(e.org, id)
            .await
            .unwrap()
            .unwrap()
            .value
            .members()
            .len(),
        3
    );
    assert!(matches!(
        svc.discard(e.org, run.id).await,
        Err(AiImportError::InvalidState(_))
    ));
}

#[tokio::test]
async fn apply_vendor_and_new_targets_in_one_transaction() {
    let e = env().await;
    let vid = e
        .vendor(
            e.org,
            "好吃便當",
            Some("02-1"),
            &[("排骨飯", 100), ("素食便當", 90)],
        )
        .await;
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 10);
    let run = proposed_run(&e, &svc).await;
    let res = svc
        .apply(
            e.org,
            run,
            ApplyCmd {
                save_photo: None,
                accepted: vec![
                    acc_vendor(
                        TargetRef::Existing { existing_id: vid },
                        // blank phone must not clear; name is never changed
                        info("改名嘗試", Some(" "), Some("台北市")),
                        &[("排骨飯", Some(105)), ("雞腿飯", Some(120))],
                    ),
                    acc_vendor(
                        TargetRef::New {
                            name: "ignored".into(),
                        },
                        info("新店", Some("03-3"), None),
                        &[("A餐", Some(130))],
                    ),
                    acc_roster(
                        TargetRef::New {
                            name: "新名單".into(),
                        },
                        &[("甲", None), ("乙", Some("n"))],
                    ),
                ],
            },
        )
        .await
        .unwrap();
    assert_eq!(res.vendor_ids.len(), 2);
    assert_eq!(res.roster_ids.len(), 1);

    let v = e.vendors.find(e.org, vid).await.unwrap().unwrap().value;
    assert_eq!(v.name().as_str(), "好吃便當");
    assert_eq!(v.phone(), Some("02-1"));
    assert_eq!(v.address(), Some("台北市"));
    let items: Vec<_> = v
        .menu()
        .iter()
        .map(|m| (m.name.as_str(), m.price.get()))
        .collect();
    assert_eq!(items, [("排骨飯", 105), ("素食便當", 90), ("雞腿飯", 120)]);

    let nv = e
        .vendors
        .find(e.org, res.vendor_ids[1])
        .await
        .unwrap()
        .unwrap()
        .value;
    assert_eq!(
        (nv.name().as_str(), nv.phone(), nv.menu().len()),
        ("新店", Some("03-3"), 1)
    );
    let nr = e
        .rosters
        .find(e.org, res.roster_ids[0])
        .await
        .unwrap()
        .unwrap()
        .value;
    assert_eq!((nr.name().as_str(), nr.members().len()), ("新名單", 2));
}

#[tokio::test]
async fn apply_new_vendor_whose_name_now_exists_updates_it() {
    let e = env().await;
    let vid = e.vendor(e.org, "好吃便當", None, &[("排骨飯", 100)]).await;
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 10);
    let run = proposed_run(&e, &svc).await;
    let res = svc
        .apply(
            e.org,
            run,
            ApplyCmd {
                save_photo: None,
                accepted: vec![acc_vendor(
                    TargetRef::New { name: "x".into() },
                    info("好吃便當 ", None, None),
                    &[("排骨飯", Some(110))],
                )],
            },
        )
        .await
        .unwrap();
    assert_eq!(res.vendor_ids, [vid]);
    assert_eq!(e.vendors.list(e.org).await.unwrap().len(), 1);
    let v = e.vendors.find(e.org, vid).await.unwrap().unwrap().value;
    assert_eq!(v.menu()[0].price.get(), 110);
}

#[tokio::test]
async fn invalid_edited_values_give_422_list_and_write_nothing() {
    let e = env().await;
    let rid = e.roster(e.org, "業務", &[("Amy", None)]).await;
    let theirs = e.roster(e.other, "別人", &[]).await;
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 10);
    let run = proposed_run(&e, &svc).await;
    let err = svc
        .apply(
            e.org,
            run,
            ApplyCmd {
                save_photo: None,
                accepted: vec![
                    acc_roster(
                        TargetRef::Existing { existing_id: rid },
                        &[("Valid", None), (" ", None)],
                    ),
                    acc_vendor(
                        TargetRef::New { name: "x".into() },
                        info("店", None, None),
                        &[("好", Some(0)), ("缺價", None), ("也好", Some(5))],
                    ),
                    acc_roster(
                        TargetRef::Existing {
                            existing_id: theirs,
                        },
                        &[("X", None)],
                    ),
                    acc_roster(TargetRef::New { name: "".into() }, &[]),
                ],
            },
        )
        .await
        .unwrap_err();
    let AiImportError::InvalidChanges(list) = err else {
        panic!("{err:?}")
    };
    let paths: Vec<_> = list.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "accepted[0].members[1].name",
            "accepted[1].menu[0].price",
            "accepted[1].menu[1].price",
            "accepted[2].roster",
            "accepted[3].roster.name",
        ]
    );
    assert!(list[2].message.contains("必填"));
    // nothing written, run still proposed
    assert_eq!(
        e.rosters
            .find(e.org, rid)
            .await
            .unwrap()
            .unwrap()
            .value
            .members()
            .len(),
        1
    );
    assert!(e.vendors.list(e.org).await.unwrap().is_empty());
    assert_eq!(
        svc.get(e.org, run).await.unwrap().status,
        ImportRunStatus::Proposed
    );
}

#[tokio::test]
async fn failure_in_a_later_step_rolls_back_earlier_steps_and_the_run() {
    let e = env().await;
    let items: Vec<_> = (0..200).map(|i| (format!("i{i}"), 10i64)).collect();
    let refs: Vec<(&str, i64)> = items.iter().map(|(n, p)| (n.as_str(), *p)).collect();
    let full = e.vendor(e.org, "滿店", None, &refs).await;
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 10);
    let run = proposed_run(&e, &svc).await;
    let err = svc
        .apply(
            e.org,
            run,
            ApplyCmd {
                save_photo: None,
                accepted: vec![
                    acc_roster(
                        TargetRef::New {
                            name: "會被回滾".into(),
                        },
                        &[("A", None)],
                    ),
                    acc_vendor(
                        TargetRef::Existing { existing_id: full },
                        None,
                        &[("多出來的", Some(5))],
                    ),
                ],
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            AiImportError::Domain(domain::shared::DomainError::LimitExceeded { .. })
        ),
        "{err:?}"
    );
    assert!(e.rosters.list(e.org).await.unwrap().is_empty());
    assert_eq!(
        svc.get(e.org, run).await.unwrap().status,
        ImportRunStatus::Proposed
    );
    // and the run can still be applied correctly afterwards
    svc.apply(
        e.org,
        run,
        ApplyCmd {
            save_photo: None,
            accepted: vec![acc_roster(TargetRef::New { name: "ok".into() }, &[])],
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn discard_and_ownership() {
    let e = env().await;
    let svc = e.service(Arc::new(FakeImportAgent::empty()), 10);
    let run = proposed_run(&e, &svc).await;
    assert!(matches!(
        svc.discard(e.other, run).await,
        Err(AiImportError::NotFound)
    ));
    assert!(matches!(
        svc.apply(e.other, run, ApplyCmd::default()).await,
        Err(AiImportError::NotFound)
    ));
    let d = svc.discard(e.org, run).await.unwrap();
    assert_eq!(d.status, ImportRunStatus::Discarded);
    assert!(matches!(
        svc.discard(e.org, run).await,
        Err(AiImportError::InvalidState(_))
    ));
    assert!(matches!(
        svc.apply(e.org, run, ApplyCmd::default()).await,
        Err(AiImportError::InvalidState(_))
    ));
    assert_eq!(
        svc.get(e.org, run).await.unwrap().status,
        ImportRunStatus::Discarded
    );
}

#[test]
fn accepted_change_json_shape() {
    let c: AcceptedChange = serde_json::from_value(json!({
        "type": "vendor",
        "vendor": { "kind": "new", "name": "好吃便當" },
        "info": { "name": "好吃便當", "phone": null },
        "menu": [{ "name": "滷肉飯", "price": 60 }]
    }))
    .unwrap();
    let AcceptedChange::Vendor(v) = c else {
        panic!()
    };
    assert_eq!(v.menu[0].price, Some(60));
    assert_eq!(v.menu[0].description, None);
    let c: AcceptedChange = serde_json::from_value(json!({
        "type": "roster",
        "roster": { "kind": "existing", "existingId": RosterId::new().to_string() },
        "members": [{ "name": "A" }]
    }))
    .unwrap();
    assert!(matches!(c, AcceptedChange::Roster(_)));
}

struct Recording {
    fail: bool,
}

#[async_trait]
impl ImportAgent for Recording {
    async fn run(
        &self,
        ctx: Arc<ImportToolContext>,
        _i: AgentInput,
    ) -> Result<AgentOutcome, AiError> {
        ctx.add_usage(Usage {
            input_tokens: 500,
            output_tokens: 20,
        });
        ctx.record_tool_call("list_vendors", json!({}));
        if self.fail {
            return Err(AiError::Timeout);
        }
        Ok(AgentOutcome {
            summary: "ok".into(),
            usage: Usage {
                input_tokens: 500,
                output_tokens: 20,
            },
            model: "m".into(),
        })
    }
}

#[tokio::test]
async fn tool_calls_and_usage_are_persisted_also_for_failed_runs() {
    let e = env().await;
    for fail in [false, true] {
        let svc = e.service(Arc::new(Recording { fail }), 10);
        let id = match svc.start(e.org, cmd()).await {
            Ok(run) => run.id,
            Err(AiImportError::Agent { run_id, .. }) => run_id,
            Err(other) => panic!("{other}"),
        };
        let run = svc.get(e.org, id).await.unwrap();
        assert_eq!(run.usage.input_tokens, 500, "fail={fail}");
        assert_eq!(run.usage.output_tokens, 20);
        let calls: serde_json::Value =
            serde_json::from_str(run.tool_calls.as_deref().unwrap()).unwrap();
        assert_eq!(calls[0]["name"], "list_vendors");
        assert_eq!(
            run.status,
            if fail {
                ImportRunStatus::Failed
            } else {
                ImportRunStatus::Proposed
            }
        );
    }
}
