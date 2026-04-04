//! i said i aint doing ts no more why am i still here
#![cfg(unix)]
use futures::TryStreamExt;

use crate::subcommands::address::AddrOutput;

// shit this one is even worse you needa collect info from two places
pub async fn get(dev: Option<&str>) -> anyhow::Result<Vec<AddrOutput>> {
    let (conn, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(conn); // every time?
    let mut address = handle.address().get();
    let mut link = handle.link().get();
    if let Some(dev) = dev {
        link = link.match_name(dev.to_owned());
        if let Some(ind) = link.execute().try_next().await?.map(|a| a.header.index) {
            address = address.set_link_index_filter(ind);
        };
        address.execute().try_next().await?;
    } else {
    };
    todo!();
}
