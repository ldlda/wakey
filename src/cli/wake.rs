use anyhow::Result;
use tracing::debug;
use wakey::WakeInput;

use super::{WakeArgs, table};

pub(super) async fn run(args: WakeArgs) -> Result<()> {
    let as_json = args.json;
    debug!(
        has_query = args.query.is_some(),
        has_mac = args.mac.is_some(),
        has_ip = args.ip.is_some(),
        json = as_json,
        "dispatching wake command"
    );
    let result = WakeInput::from_parts(args.query, args.mac, args.ip)?
        .send()
        .await?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        println!("{}", table::render_wake_table(&result));
    }
    Ok(())
}
