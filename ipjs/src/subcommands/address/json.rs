//        ip address [ show [ dev IFNAME ] [ scope SCOPE-ID ] [ master DEVICE ]
//                          [ type TYPE ] [ to PREFIX ] [ FLAG-LIST ]
//                          [ label LABEL ] [up] [ vrf NAME ] ]
// fuck is this mean

// do i need all this? do i need anything but `ip -j a show dev br-lan`?

// TYPE := { vlan | veth | vcan | vxcan | dummy | ifb | macvlan | macvtap |
//           bridge | bond | ipoib | ip6tnl | ipip | sit | vxlan | lowpan |
//           gre | gretap | erspan | ip6gre | ip6gretap | ip6erspan | vti |
//           nlmon | can | bond_slave | ipvlan | geneve | bridge_slave |
//           hsr | macsec | netdevsim }
// FLAG-LIST := [ FLAG-LIST ] FLAG
// FLAG  := [ permanent | dynamic | secondary | primary |
//            [-]tentative | [-]deprecated | [-]dadfailed | temporary |
//            CONFFLAG-LIST ]
// CONFFLAG-LIST := [ CONFFLAG-LIST ] CONFFLAG
// CONFFLAG  := [ home | nodad | mngtmpaddr | noprefixroute | autojoin ]

// prefix seems to be a cidr. both 6 and 4 works. idfk dog

use super::AddrOutput;

pub async fn get(dev: Option<&str>) -> anyhow::Result<Vec<AddrOutput>> {
    let mut cmd = tokio::process::Command::new("ip");
    cmd.args(["-j", "address", "show"]);

    if let Some(d) = dev {
        cmd.args(["dev", d]);
    }

    let output = cmd.output().await?;

    if !output.status.success() {
        anyhow::bail!(String::from_utf8_lossy(&output.stderr).into_owned());
    }

    Ok(serde_json::from_slice(&output.stdout)?)
}
