//! Manual evaluation of the AI import agent (plan T4.4). Not part of CI.
//!
//! ```text
//! cargo run -p server --bin ai_eval -- [--cases DIR] [--filter TEXT]
//!                                      [--repeat N] [--timeout SECS] [--out report.json]
//! ```
//!
//! Uses the same settings as the server (`ANTHROPIC_API_KEY`, `ANTHROPIC_BASE_URL`,
//! `AI_MODEL`, `AI_PROVIDER`, `AI_*`), so it measures exactly what production runs.
//! Cases live in `eval/cases/<name>/{image.png|image.jpg, expected.json}`.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use infrastructure::ai::RigImportAgent;
use server::ai_eval::{CaseRun, load_cases, run_case, table};
use server::config::Config;

struct Args {
    cases: PathBuf,
    filter: Option<String>,
    repeat: u32,
    timeout: Duration,
    out: Option<PathBuf>,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut a = Args {
        cases: PathBuf::from("../eval/cases"),
        filter: None,
        repeat: 1,
        timeout: Duration::from_secs(300),
        out: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = |name: &str| it.next().with_context(|| format!("{name} needs a value"));
        match flag.as_str() {
            "--cases" => a.cases = value("--cases")?.into(),
            "--filter" => a.filter = Some(value("--filter")?),
            "--repeat" => a.repeat = value("--repeat")?.parse().context("--repeat")?,
            "--timeout" => {
                a.timeout = Duration::from_secs(value("--timeout")?.parse().context("--timeout")?)
            }
            "--out" => a.out = Some(value("--out")?.into()),
            "-h" | "--help" => {
                println!(
                    "usage: ai_eval [--cases DIR] [--filter TEXT] [--repeat N] [--timeout SECS] [--out report.json]"
                );
                std::process::exit(0);
            }
            other => bail!("unknown argument {other}"),
        }
    }
    Ok(a)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = parse_args()?;
    let config = Config::from_env().context("loading config")?;
    let Some(cfg) = server::wiring::rig_agent_config(&config) else {
        bail!("ANTHROPIC_API_KEY is not set (make ai-eval maps STRATA_* onto ANTHROPIC_*)");
    };
    // Never prints the key: RigAgentConfig's Debug redacts it.
    eprintln!("agent: {cfg:?}");
    let agent = RigImportAgent::new(cfg).map_err(|e| anyhow::anyhow!(e))?;

    let cases = load_cases(&args.cases, args.filter.as_deref()).map_err(|e| anyhow::anyhow!(e))?;
    if cases.is_empty() {
        bail!("no cases found in {}", args.cases.display());
    }
    let mut runs: Vec<CaseRun> = Vec::new();
    for case in &cases {
        for n in 1..=args.repeat {
            eprintln!("running {} (#{n}) ...", case.name);
            let run = run_case(&agent, case, n, args.timeout)
                .await
                .map_err(|e| anyhow::anyhow!("{}: {e}", case.name))?;
            eprintln!(
                "  {} in {:.1}s, {} turns",
                run.outcome.label(),
                run.latency_ms as f64 / 1000.0,
                run.turns
            );
            runs.push(run);
        }
    }
    println!("\n{}", table(&runs));

    if let Some(path) = &args.out {
        let report = serde_json::json!({
            "model": config.ai_model,
            "provider": format!("{:?}", config.ai_provider),
            "strictTools": config.ai_strict_tools,
            "reasoning": format!("{:?}", config.ai_reasoning),
            "fallbacks": config.ai_fallbacks,
            "runs": runs,
        });
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).ok();
        }
        std::fs::write(path, serde_json::to_string_pretty(&report)?)
            .with_context(|| format!("writing {}", path.display()))?;
        eprintln!("report written to {}", path.display());
    }
    Ok(())
}
