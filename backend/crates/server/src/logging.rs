//! Shared production formatter; callers may supply a scoped writer/subscriber in tests.

use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

use crate::config::LogFormat;

pub fn env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,tower_http=info"))
}

pub fn subscriber<W>(
    format: LogFormat,
    filter: EnvFilter,
    writer: W,
) -> impl tracing::Subscriber + Send + Sync
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer);
    match format {
        LogFormat::Text => Box::new(builder.finish()) as Box<dyn tracing::Subscriber + Send + Sync>,
        // Keep event fields nested: names cannot collide with timestamp/level/span metadata.
        LogFormat::Json => Box::new(builder.json().with_ansi(false).finish()),
    }
}
