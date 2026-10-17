//! The `server admin ...` subcommands: managing local (password) accounts
//! without the web UI. `run_admin` takes the password source and the output
//! as parameters so it can be tested without a terminal.

use std::io::{BufRead, Write};

use application::ports::AccountSummary;
use application::{AccountError, AccountService};
use clap::{Parser, Subcommand};
use infrastructure::timefmt::to_text;

/// Order-a-bento server. Without a subcommand it starts the HTTP server.
#[derive(Debug, Parser)]
#[command(name = "server", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage local admin accounts (email + password) without the web UI.
    ///
    /// Reads the same configuration and `.env` as the server and migrates DATABASE_URL first.
    #[command(subcommand)]
    Admin(AdminCommand),
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// Create a local account. Fails if a local account with the email exists.
    Create {
        #[arg(long)]
        email: String,
        /// Display name shown in the UI.
        #[arg(long)]
        name: String,
        /// Read the password as one line from stdin instead of prompting.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Replace the password of an existing local account.
    SetPassword {
        #[arg(long)]
        email: String,
        /// Read the password as one line from stdin instead of prompting.
        #[arg(long)]
        password_stdin: bool,
    },
    /// List accounts (email, name, login methods, creation time); never shows hashes.
    List,
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Bad email, weak password, mismatching confirmation ...
    #[error("{0}")]
    Invalid(String),
    #[error(
        "a local account with email {0} already exists (use `admin set-password` to change its password)"
    )]
    AlreadyExists(String),
    #[error("no local account with email {0}")]
    NotFound(String),
    #[error("{0}")]
    Internal(String),
}

impl CliError {
    /// Process exit code (0 = success, 2 = usage errors reported by clap).
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Internal(_) => 1,
            Self::Invalid(_) => 3,
            Self::AlreadyExists(_) => 4,
            Self::NotFound(_) => 5,
        }
    }
}

impl From<std::io::Error> for CliError {
    fn from(e: std::io::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

fn account_error(e: AccountError, email: &str) -> CliError {
    match e {
        AccountError::Invalid(d) => CliError::Invalid(d.to_string()),
        AccountError::EmailTaken => CliError::AlreadyExists(email.trim().to_lowercase()),
        AccountError::NotFound => CliError::NotFound(email.trim().to_lowercase()),
        AccountError::Internal(d) => CliError::Internal(d),
    }
}

/// One line from `input` without its trailing `\n` / `\r\n` (nothing else is trimmed).
pub fn read_password_line(input: &mut impl BufRead) -> Result<String, CliError> {
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Err(CliError::Invalid("no password on stdin".into()));
    }
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(line)
}

/// Where the password comes from.
pub trait PasswordSource {
    fn read(&mut self) -> Result<String, CliError>;
}

/// `--password-stdin`: a single line from stdin.
pub struct StdinPassword;

impl PasswordSource for StdinPassword {
    fn read(&mut self) -> Result<String, CliError> {
        read_password_line(&mut std::io::stdin().lock())
    }
}

/// Interactive no-echo prompt, asked twice.
pub struct PromptPassword;

impl PasswordSource for PromptPassword {
    fn read(&mut self) -> Result<String, CliError> {
        let first = rpassword::prompt_password("Password: ")?;
        let second = rpassword::prompt_password("Confirm password: ")?;
        if first != second {
            return Err(CliError::Invalid("passwords do not match".into()));
        }
        Ok(first)
    }
}

/// The password source selected by `--password-stdin`.
pub fn password_source(stdin: bool) -> Box<dyn PasswordSource> {
    if stdin {
        Box::new(StdinPassword)
    } else {
        Box::new(PromptPassword)
    }
}

fn methods(a: &AccountSummary) -> String {
    match (a.has_google, a.has_password) {
        (true, true) => "google,password",
        (true, false) => "google",
        (false, true) => "password",
        (false, false) => "-",
    }
    .into()
}

/// Run one `admin` subcommand, writing user-facing output to `out`.
pub async fn run_admin(
    svc: &AccountService,
    cmd: &AdminCommand,
    password: &mut dyn PasswordSource,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    match cmd {
        AdminCommand::Create { email, name, .. } => {
            // Fail on a bad email/name before asking for a password.
            domain::identity::Organizer::new_local(email, name)
                .map_err(|e| CliError::Invalid(e.to_string()))?;
            let pw = password.read()?;
            let o = svc
                .create(email, name, &pw)
                .await
                .map_err(|e| account_error(e, email))?;
            writeln!(out, "Created account {} (id {})", o.email, o.id)?;
        }
        AdminCommand::SetPassword { email, .. } => {
            domain::identity::parse_local_email(email)
                .map_err(|e| CliError::Invalid(e.to_string()))?;
            let pw = password.read()?;
            svc.set_password(email, &pw)
                .await
                .map_err(|e| account_error(e, email))?;
            writeln!(out, "Password updated for {}", email.trim().to_lowercase())?;
        }
        AdminCommand::List => {
            let rows = svc.list().await.map_err(|e| account_error(e, ""))?;
            let w = |f: fn(&AccountSummary) -> usize, head: usize| {
                rows.iter().map(f).max().unwrap_or(0).max(head)
            };
            let (we, wn, wm) = (
                w(|a| a.email.chars().count(), 5),
                w(|a| a.display_name.chars().count(), 4),
                w(|a| methods(a).len(), 7),
            );
            writeln!(
                out,
                "{:we$}  {:wn$}  {:wm$}  CREATED",
                "EMAIL", "NAME", "METHODS"
            )?;
            for a in &rows {
                writeln!(
                    out,
                    "{:we$}  {:wn$}  {:wm$}  {}",
                    a.email,
                    a.display_name,
                    methods(a),
                    to_text(a.created_at)
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use application::ports::PortResult;
    use infrastructure::sqlite::Db;

    use super::*;

    struct Fixed(&'static str);
    impl PasswordSource for Fixed {
        fn read(&mut self) -> Result<String, CliError> {
            Ok(self.0.to_owned())
        }
    }

    /// A source that must not be asked.
    struct Never;
    impl PasswordSource for Never {
        fn read(&mut self) -> Result<String, CliError> {
            panic!("password requested");
        }
    }

    async fn run(
        svc: &AccountService,
        cmd: AdminCommand,
        pw: &mut dyn PasswordSource,
    ) -> (Result<(), CliError>, String) {
        let mut out = Vec::new();
        let r = run_admin(svc, &cmd, pw, &mut out).await;
        (r, String::from_utf8(out).unwrap())
    }

    fn create(email: &str) -> AdminCommand {
        AdminCommand::Create {
            email: email.into(),
            name: "Root".into(),
            password_stdin: true,
        }
    }

    #[tokio::test]
    async fn create_set_password_and_list() {
        let (db, _d) = Db::open_temp().await.unwrap();
        let svc = crate::wiring::account_service(&db);

        let (r, out) = run(
            &svc,
            create("Admin@Example.com"),
            &mut Fixed("correct horse"),
        )
        .await;
        r.unwrap();
        assert!(
            out.starts_with("Created account admin@example.com (id "),
            "{out}"
        );

        // Duplicate (case-insensitive) -> exit code 4; weak password -> 3; bad email -> 3 before prompting.
        let (r, _) = run(
            &svc,
            create("ADMIN@example.com"),
            &mut Fixed("correct horse"),
        )
        .await;
        let e = r.unwrap_err();
        assert!(matches!(e, CliError::AlreadyExists(_)));
        assert_eq!(e.exit_code(), 4);
        let (r, _) = run(&svc, create("b@example.com"), &mut Fixed("short")).await;
        assert_eq!(r.unwrap_err().exit_code(), 3);
        let (r, _) = run(&svc, create("not-an-email"), &mut Never).await;
        assert_eq!(r.unwrap_err().exit_code(), 3);

        let set = |email: &str| AdminCommand::SetPassword {
            email: email.into(),
            password_stdin: true,
        };
        let (r, out) = run(
            &svc,
            set("admin@example.com"),
            &mut Fixed("brand new password"),
        )
        .await;
        r.unwrap();
        assert_eq!(out, "Password updated for admin@example.com\n");
        let (r, _) = run(
            &svc,
            set("nobody@example.com"),
            &mut Fixed("brand new password"),
        )
        .await;
        assert_eq!(r.unwrap_err().exit_code(), 5);

        // The stored hash is the new one (verified through the real login path).
        let repo = infrastructure::identity_repo::SqliteOrganizerRepo::new(db.clone());
        let hasher = infrastructure::password::Argon2PasswordHasher::new();
        let hash =
            application::ports::OrganizerRepo::find_local_by_email(&repo, "admin@example.com")
                .await
                .unwrap()
                .unwrap()
                .password_hash
                .unwrap();
        use application::ports::PasswordHasher as _;
        let ok: PortResult<bool> = hasher.verify("brand new password", &hash).await;
        assert!(ok.unwrap());

        let (r, out) = run(&svc, AdminCommand::List, &mut Never).await;
        r.unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].starts_with("EMAIL") && lines[0].contains("METHODS"));
        assert!(lines[1].starts_with("admin@example.com"));
        assert!(lines[1].contains("Root") && lines[1].contains("password"));
        assert!(!out.contains("$argon2"), "hashes must not be printed");
    }

    #[test]
    fn stdin_password_strips_only_the_line_ending() {
        for (input, want) in [
            ("secret pass\n", "secret pass"),
            ("secret pass\r\n", "secret pass"),
            ("secret pass", "secret pass"),
            (" padded pass \nsecond line\n", " padded pass "),
        ] {
            assert_eq!(read_password_line(&mut Cursor::new(input)).unwrap(), want);
        }
        assert_eq!(
            read_password_line(&mut Cursor::new(""))
                .unwrap_err()
                .exit_code(),
            3
        );
    }

    #[test]
    fn clap_parses_commands() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
        let c = Cli::try_parse_from(["server"]).unwrap();
        assert!(c.command.is_none());
        let c = Cli::try_parse_from([
            "server",
            "admin",
            "create",
            "--email",
            "a@b.c",
            "--name",
            "A B",
            "--password-stdin",
        ])
        .unwrap();
        assert!(matches!(
            c.command,
            Some(Command::Admin(AdminCommand::Create {
                password_stdin: true,
                ..
            }))
        ));
        assert!(Cli::try_parse_from(["server", "admin", "create", "--email", "a@b.c"]).is_err());
        assert!(Cli::try_parse_from(["server", "admin", "list"]).is_ok());
    }
}
