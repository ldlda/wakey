use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init(verbose: u8) {
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(default_filter(verbose)))
        .expect("static tracing filter should parse");

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();
}

fn default_filter(verbose: u8) -> &'static str {
    match verbose {
        0 => "wakey_agent=info,wakey=info",
        1 => "wakey_agent=debug,wakey=debug",
        _ => "wakey_agent=trace,wakey=debug",
    }
}
