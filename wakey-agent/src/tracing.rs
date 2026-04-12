use time::UtcOffset;
use time::format_description::well_known::Rfc3339;
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init(verbose: u8) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_filter(verbose)))
        .expect("static tracing filter should parse");

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_timer(local_offset_timer()))
        .init();
}

fn default_filter(verbose: u8) -> &'static str {
    match verbose {
        0 => "wakey_agent=info,wakey=info",
        1 => "wakey_agent=debug,wakey=debug",
        _ => "wakey_agent=trace,wakey=debug",
    }
}

fn local_offset_timer() -> OffsetTime<Rfc3339> {
    let offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    OffsetTime::new(offset, Rfc3339)
}
