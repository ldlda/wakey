#![cfg(unix)]

use futures::TryStreamExt;
use rtnetlink::Handle;
use rtnetlink::packet_route::link::LinkAttribute;

use super::LinkOutput;

pub async fn get(dev: Option<&str>) -> anyhow::Result<Vec<LinkOutput>> {
    let (conn, handle, _) = rtnetlink::new_connection()?;
    tokio::spawn(conn);

    get_with_handle(&handle, dev).await
}

pub async fn get_with_handle(
    handle: &Handle,
    dev: Option<&str>,
) -> anyhow::Result<Vec<LinkOutput>> {
    let mut req = handle.link().get();
    if let Some(dev) = dev {
        req = req.match_name(dev.to_owned());
    }

    let mut stream = req.execute();
    let mut out = Vec::new();

    while let Some(link) = stream.try_next().await? {
        let mut ifname = None;
        let mut operstate = None;
        let mut address = None;

        for attr in link.attributes {
            match attr {
                LinkAttribute::IfName(name) => ifname = Some(name),
                LinkAttribute::Address(bytes) => {
                    address = match bytes.len() {
                        6 => bytes.first_chunk::<6>().map(|&b| macaddr::MacAddr::from(b)),
                        8 => bytes.first_chunk::<8>().map(|&b| macaddr::MacAddr::from(b)),
                        _ => None,
                    }
                }
                LinkAttribute::OperState(state) => operstate = Some(state.into()),
                _ => {}
            }
        }

        if let Some(ifname) = ifname {
            out.push(LinkOutput {
                ifindex: link.header.index,
                ifname,
                operstate,
                address,
            });
        }
    }

    Ok(out)
}
