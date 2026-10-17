use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use domain::identity::OrganizerId;

use super::accounts::{AccountError, AccountService, BootstrapOutcome};
use super::*;
use crate::ports::{
    AccountSummary, CreateLocalOutcome, IdentityError, LocalAccount, PortResult, VerifiedIdentity,
};

#[derive(Default)]
struct FakeOrganizers(Mutex<Vec<(Organizer, Option<String>)>>);

#[async_trait]
impl OrganizerRepo for FakeOrganizers {
    async fn find_by_google_sub(&self, sub: &str) -> PortResult<Option<Organizer>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|(o, _)| o.google_sub.as_deref() == Some(sub))
            .map(|(o, _)| o.clone()))
    }
    async fn find_by_id(&self, id: OrganizerId) -> PortResult<Option<Organizer>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|(o, _)| o.id == id)
            .map(|(o, _)| o.clone()))
    }
    async fn upsert_from_identity(&self, c: Organizer, _: SystemTime) -> PortResult<Organizer> {
        let mut m = self.0.lock().unwrap();
        if let Some((o, _)) = m.iter_mut().find(|(o, _)| o.google_sub == c.google_sub) {
            *o = Organizer { id: o.id, ..c };
            return Ok(o.clone());
        }
        m.push((c.clone(), None));
        Ok(c)
    }
    async fn touch_last_login(&self, _: OrganizerId, _: SystemTime) -> PortResult<()> {
        Ok(())
    }
    async fn find_local_by_email(&self, email: &str) -> PortResult<Option<LocalAccount>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|(o, _)| o.is_local() && o.email == email)
            .map(|(o, h)| LocalAccount {
                organizer: o.clone(),
                password_hash: h.clone(),
            }))
    }
    async fn exists_by_email(&self, email: &str) -> PortResult<bool> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|(o, _)| o.email.to_lowercase() == email))
    }
    async fn create_local(
        &self,
        o: Organizer,
        hash: String,
        _: SystemTime,
    ) -> PortResult<CreateLocalOutcome> {
        let mut m = self.0.lock().unwrap();
        if m.iter().any(|(x, _)| x.is_local() && x.email == o.email) {
            return Ok(CreateLocalOutcome::EmailTaken);
        }
        m.push((o, Some(hash)));
        Ok(CreateLocalOutcome::Created)
    }
    async fn set_password_hash(&self, id: OrganizerId, hash: String) -> PortResult<bool> {
        let mut m = self.0.lock().unwrap();
        match m.iter_mut().find(|(o, _)| o.id == id) {
            Some((_, h)) => {
                *h = Some(hash);
                Ok(true)
            }
            None => Ok(false),
        }
    }
    async fn list_accounts(&self) -> PortResult<Vec<AccountSummary>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|(o, h)| AccountSummary {
                email: o.email.clone(),
                display_name: o.display_name.clone(),
                has_google: o.google_sub.is_some(),
                has_password: h.is_some(),
                created_at: SystemTime::UNIX_EPOCH,
            })
            .collect())
    }
}

/// Fast stand-in for Argon2: `hash:<password>`; counts dummy verifications.
#[derive(Default)]
struct FakeHasher {
    dummy_verifies: AtomicU64,
}

#[async_trait]
impl PasswordHasher for FakeHasher {
    async fn hash(&self, password: &str) -> PortResult<String> {
        Ok(format!("hash:{password}"))
    }
    async fn verify(&self, password: &str, hash: &str) -> PortResult<bool> {
        Ok(hash == format!("hash:{password}"))
    }
    async fn verify_dummy(&self, _: &str) {
        self.dummy_verifies.fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct FakeSessions(Mutex<HashMap<[u8; 32], (OrganizerId, SystemTime)>>);

#[async_trait]
impl SessionRepo for FakeSessions {
    async fn create(
        &self,
        h: TokenHash,
        o: OrganizerId,
        _: SystemTime,
        exp: SystemTime,
        _: Option<&str>,
    ) -> PortResult<()> {
        self.0.lock().unwrap().insert(h.0, (o, exp));
        Ok(())
    }
    async fn find_valid(
        &self,
        h: TokenHash,
        now: SystemTime,
    ) -> PortResult<Option<(OrganizerId, SystemTime)>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .get(&h.0)
            .copied()
            .filter(|(_, e)| *e > now))
    }
    async fn extend(&self, h: TokenHash, exp: SystemTime) -> PortResult<()> {
        if let Some(v) = self.0.lock().unwrap().get_mut(&h.0) {
            v.1 = exp;
        }
        Ok(())
    }
    async fn delete(&self, h: TokenHash) -> PortResult<()> {
        self.0.lock().unwrap().remove(&h.0);
        Ok(())
    }
    async fn delete_expired(&self, now: SystemTime) -> PortResult<u64> {
        let mut m = self.0.lock().unwrap();
        let before = m.len();
        m.retain(|_, (_, e)| *e > now);
        Ok((before - m.len()) as u64)
    }
}

#[derive(Default)]
struct FakeStates(Mutex<HashMap<String, OAuthState>>);

#[async_trait]
impl OAuthStateRepo for FakeStates {
    async fn save(&self, s: OAuthState) -> PortResult<()> {
        self.0.lock().unwrap().insert(s.state.clone(), s);
        Ok(())
    }
    async fn take(&self, state: &str, now: SystemTime) -> PortResult<Option<OAuthState>> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .remove(state)
            .filter(|s| s.expires_at > now))
    }
    async fn delete_expired(&self, _: SystemTime) -> PortResult<u64> {
        Ok(0)
    }
}

struct FakeIdp(Mutex<VerifiedIdentity>);

#[async_trait]
impl IdentityProvider for FakeIdp {
    async fn authorize_url(
        &self,
        state: &str,
        nonce: &str,
        ch: &str,
    ) -> Result<String, IdentityError> {
        Ok(format!(
            "https://idp.test/auth?state={state}&nonce={nonce}&cc={ch}"
        ))
    }
    async fn exchange(
        &self,
        code: &str,
        _: &str,
        _: &str,
    ) -> Result<VerifiedIdentity, IdentityError> {
        if code == "bad" {
            return Err(IdentityError::Provider("bad code".into()));
        }
        Ok(self.0.lock().unwrap().clone())
    }
}

struct Counter(AtomicU64);
impl TokenGenerator for Counter {
    fn generate(&self) -> String {
        format!("tok{}", self.0.fetch_add(1, Ordering::Relaxed))
    }
}

struct TestClock(Mutex<SystemTime>);
impl Clock for TestClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}

const DAY: Duration = Duration::from_secs(86_400);

struct Fixture {
    svc: AuthService,
    clock: Arc<TestClock>,
    sessions: Arc<FakeSessions>,
    idp: Arc<FakeIdp>,
    organizers: Arc<FakeOrganizers>,
    hasher: Arc<FakeHasher>,
}

fn fixture() -> Fixture {
    let clock = Arc::new(TestClock(Mutex::new(SystemTime::UNIX_EPOCH + 1000 * DAY)));
    let sessions = Arc::new(FakeSessions::default());
    let idp = Arc::new(FakeIdp(Mutex::new(VerifiedIdentity {
        sub: "g-1".into(),
        email: "a@b.c".into(),
        email_verified: true,
        name: Some("Alice".into()),
        picture: None,
    })));
    let organizers = Arc::new(FakeOrganizers::default());
    let hasher = Arc::new(FakeHasher::default());
    let svc = AuthService {
        organizers: organizers.clone(),
        sessions: sessions.clone(),
        oauth_states: Arc::new(FakeStates::default()),
        identity: Some(idp.clone()),
        hasher: hasher.clone(),
        tokens: Arc::new(Counter(AtomicU64::new(0))),
        clock: clock.clone(),
        config: AuthConfig::from_days(14),
    };
    Fixture {
        svc,
        clock,
        sessions,
        idp,
        organizers,
        hasher,
    }
}

fn accounts(f: &Fixture) -> AccountService {
    AccountService {
        organizers: f.organizers.clone(),
        hasher: f.hasher.clone(),
        clock: f.clock.clone(),
    }
}

fn state_of(url: &str) -> String {
    url.split("state=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn return_to_validation() {
    assert_eq!(sanitize_return_to(None), "/");
    assert_eq!(
        sanitize_return_to(Some("/admin/orders?x=1")),
        "/admin/orders?x=1"
    );
    for bad in [
        "//evil.com",
        "https://evil.com",
        "/\\evil",
        "evil",
        "",
        "/a\\b",
        "/a\nb",
    ] {
        assert_eq!(sanitize_return_to(Some(bad)), "/", "{bad:?}");
    }
}

#[tokio::test]
async fn begin_and_complete_flow() {
    let f = fixture();
    let url = f.svc.begin_google_login(Some("/admin")).await.unwrap();
    let state = state_of(&url);
    let done = f
        .svc
        .complete_google_login(&state, "code", Some("UA"))
        .await
        .unwrap();
    assert_eq!(done.return_to, "/admin");
    assert_eq!(done.session.expires_at, f.clock.now() + 14 * DAY);
    let r = f.svc.resolve_session(&done.session.token).await.unwrap();
    assert_eq!(r.organizer.email, "a@b.c");
    assert_eq!(r.organizer.display_name, "Alice");
}

#[tokio::test]
async fn open_redirect_return_to_defaults() {
    let f = fixture();
    for bad in ["//evil.com", "https://evil.com", "/\\evil"] {
        let url = f.svc.begin_google_login(Some(bad)).await.unwrap();
        let done = f
            .svc
            .complete_google_login(&state_of(&url), "c", None)
            .await
            .unwrap();
        assert_eq!(done.return_to, "/");
    }
}

#[tokio::test]
async fn state_is_one_time() {
    let f = fixture();
    let state = state_of(&f.svc.begin_google_login(None).await.unwrap());
    f.svc
        .complete_google_login(&state, "c", None)
        .await
        .unwrap();
    let again = f.svc.complete_google_login(&state, "c", None).await;
    assert!(matches!(again, Err(AuthError::InvalidState)));
    let unknown = f.svc.complete_google_login("nope", "c", None).await;
    assert!(matches!(unknown, Err(AuthError::InvalidState)));
}

#[tokio::test]
async fn expired_state_rejected() {
    let f = fixture();
    let state = state_of(&f.svc.begin_google_login(None).await.unwrap());
    *f.clock.0.lock().unwrap() += OAUTH_STATE_TTL + Duration::from_secs(1);
    let r = f.svc.complete_google_login(&state, "c", None).await;
    assert!(matches!(r, Err(AuthError::InvalidState)));
}

#[tokio::test]
async fn unverified_email_rejected() {
    let f = fixture();
    f.idp.0.lock().unwrap().email_verified = false;
    let state = state_of(&f.svc.begin_google_login(None).await.unwrap());
    let r = f.svc.complete_google_login(&state, "c", None).await;
    assert!(matches!(r, Err(AuthError::EmailNotVerified)));
}

#[tokio::test]
async fn provider_failure_and_missing_provider() {
    let f = fixture();
    let state = state_of(&f.svc.begin_google_login(None).await.unwrap());
    let r = f.svc.complete_google_login(&state, "bad", None).await;
    assert!(matches!(r, Err(AuthError::Provider(_))));

    let mut svc = f.svc.clone();
    svc.identity = None;
    assert!(matches!(
        svc.begin_google_login(None).await,
        Err(AuthError::GoogleNotConfigured)
    ));
}

#[tokio::test]
async fn dev_login_creates_dev_organizer() {
    let f = fixture();
    let s = f.svc.dev_login("x@y.z", None, None).await.unwrap();
    let r = f.svc.resolve_session(&s.token).await.unwrap();
    assert_eq!(r.organizer.google_sub.as_deref(), Some("dev:x@y.z"));
    assert_eq!(r.organizer.display_name, "x");
    // Logging in again keeps the same organizer.
    let s2 = f
        .svc
        .dev_login("x@y.z", Some("Xavier"), None)
        .await
        .unwrap();
    let r2 = f.svc.resolve_session(&s2.token).await.unwrap();
    assert_eq!(r2.organizer.id, r.organizer.id);
    assert!(f.svc.dev_login("nope", None, None).await.is_err());
}

#[tokio::test]
async fn sliding_expiry_only_past_half_life() {
    let f = fixture();
    let s = f.svc.dev_login("x@y.z", None, None).await.unwrap();
    *f.clock.0.lock().unwrap() += 6 * DAY; // 8 of 14 days remain
    let r = f.svc.resolve_session(&s.token).await.unwrap();
    assert!(!r.extended);
    assert_eq!(r.expires_at, s.expires_at);
    *f.clock.0.lock().unwrap() += 2 * DAY; // 6 of 14 days remain
    let r = f.svc.resolve_session(&s.token).await.unwrap();
    assert!(r.extended);
    assert_eq!(r.expires_at, f.clock.now() + 14 * DAY);
    let r = f.svc.resolve_session(&s.token).await.unwrap();
    assert!(!r.extended);
}

#[tokio::test]
async fn expired_session_and_logout() {
    let f = fixture();
    let s = f.svc.dev_login("x@y.z", None, None).await.unwrap();
    f.svc.logout(&s.token).await.unwrap();
    assert!(matches!(
        f.svc.resolve_session(&s.token).await,
        Err(AuthError::Unauthenticated)
    ));
    f.svc.logout(&s.token).await.unwrap(); // idempotent

    let s = f.svc.dev_login("x@y.z", None, None).await.unwrap();
    *f.clock.0.lock().unwrap() += 15 * DAY;
    assert!(matches!(
        f.svc.resolve_session(&s.token).await,
        Err(AuthError::Unauthenticated)
    ));
    assert!(f.sessions.0.lock().unwrap().len() == 1);
    f.svc.purge_expired().await.unwrap();
    assert!(f.sessions.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn password_login_success_and_uniform_failures() {
    let f = fixture();
    accounts(&f)
        .create(" Admin@Example.com ", "Root", "correct horse")
        .await
        .unwrap();

    // Email matching is case/space insensitive.
    let s = f
        .svc
        .password_login("ADMIN@example.com ", "correct horse", Some("UA"))
        .await
        .unwrap();
    assert_eq!(s.expires_at, f.clock.now() + 14 * DAY);
    let r = f.svc.resolve_session(&s.token).await.unwrap();
    assert_eq!(r.organizer.email, "admin@example.com");
    assert_eq!(f.hasher.dummy_verifies.load(Ordering::Relaxed), 0);

    // Wrong password, unknown email and over-long input look the same.
    for (e, p) in [
        ("admin@example.com", "wrong password"),
        ("nobody@example.com", "correct horse"),
        ("admin@example.com", &"x".repeat(300)),
    ] {
        let err = f.svc.password_login(e, p, None).await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials), "{e}");
    }
    // The unknown account paid for a dummy verification (timing equalised).
    assert_eq!(f.hasher.dummy_verifies.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn password_login_ignores_google_accounts_and_passwordless_locals() {
    let f = fixture();
    // A Google account sharing the email is never a password-login target.
    f.svc.dev_login("g@x.y", None, None).await.unwrap();
    let err = f
        .svc
        .password_login("g@x.y", "anything at all", None)
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::InvalidCredentials));

    let o = Organizer::new_local("np@x.y", "NP").unwrap();
    f.organizers.0.lock().unwrap().push((o, None));
    let err = f
        .svc
        .password_login("np@x.y", "anything at all", None)
        .await
        .unwrap_err();
    assert!(matches!(err, AuthError::InvalidCredentials));
    assert_eq!(f.hasher.dummy_verifies.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn account_create_set_password_and_list() {
    let f = fixture();
    let a = accounts(&f);
    assert!(matches!(
        a.create("bad", "N", "long enough pw").await,
        Err(AccountError::Invalid(_))
    ));
    assert!(matches!(
        a.create("a@b.c", "N", "short").await,
        Err(AccountError::Invalid(_))
    ));
    a.create("a@b.c", "Ann", "long enough pw").await.unwrap();
    assert!(matches!(
        a.create("A@B.C", "Other", "long enough pw").await,
        Err(AccountError::EmailTaken)
    ));

    a.set_password("A@b.c", "another long pw").await.unwrap();
    assert!(
        f.svc
            .password_login("a@b.c", "long enough pw", None)
            .await
            .is_err()
    );
    f.svc
        .password_login("a@b.c", "another long pw", None)
        .await
        .unwrap();
    assert!(matches!(
        a.set_password("zz@b.c", "another long pw").await,
        Err(AccountError::NotFound)
    ));
    assert!(matches!(
        a.set_password("a@b.c", "short").await,
        Err(AccountError::Invalid(_))
    ));

    // A Google account with the same email can coexist and is not a set-password target.
    f.svc.dev_login("g@b.c", None, None).await.unwrap();
    assert!(matches!(
        a.set_password("g@b.c", "another long pw").await,
        Err(AccountError::NotFound)
    ));
    let list = a.list().await.unwrap();
    assert_eq!(list.len(), 2);
    assert!(
        list.iter()
            .any(|s| s.email == "a@b.c" && s.has_password && !s.has_google)
    );
}

#[tokio::test]
async fn bootstrap_admin_is_idempotent_and_never_overwrites() {
    let f = fixture();
    let a = accounts(&f);
    assert_eq!(
        a.bootstrap_admin("Root@X.io", "first password")
            .await
            .unwrap(),
        BootstrapOutcome::Created
    );
    assert_eq!(
        a.bootstrap_admin("root@x.io", "changed password")
            .await
            .unwrap(),
        BootstrapOutcome::AlreadyExists
    );
    // The original password still works; the env value did not replace it.
    f.svc
        .password_login("root@x.io", "first password", None)
        .await
        .unwrap();
    assert!(
        f.svc
            .password_login("root@x.io", "changed password", None)
            .await
            .is_err()
    );
    // Any existing organizer with the email (here a dev/Google one) blocks creation.
    f.svc.dev_login("g@x.io", None, None).await.unwrap();
    assert_eq!(
        a.bootstrap_admin("g@x.io", "some password").await.unwrap(),
        BootstrapOutcome::AlreadyExists
    );
    assert!(matches!(
        a.bootstrap_admin("root2@x.io", "short").await,
        Err(AccountError::Invalid(_))
    ));
}
