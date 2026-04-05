//! Typed wrappers for `ip -j link show`.

pub mod json;
#[cfg(all(unix, feature = "experimental-nl"))]
pub mod nl;

pub use crate::subcommands::Backend;
use crate::utils::serialize::mac::option_mac;
use macaddr::MacAddr;
#[cfg(all(unix, feature = "experimental-nl"))]
use rtnetlink::packet_route::link::State as NetlinkOperState;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkOutput {
    pub ifindex: u32,
    pub ifname: String,
    #[serde(default)]
    pub operstate: Option<OperState>,
    #[serde(default, with = "option_mac")]
    pub address: Option<MacAddr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperState {
    Up,
    Down,
    Unknown,
    Dormant,
    LowerLayerDown,
    NotPresent,
    Testing,
    Other,
}

impl OperState {
    pub fn parse_lossy(value: &str) -> Self {
        match value.to_ascii_uppercase().as_str() {
            "UP" => Self::Up,
            "DOWN" => Self::Down,
            "UNKNOWN" => Self::Unknown,
            "DORMANT" => Self::Dormant,
            "LOWERLAYERDOWN" | "LOWER_LAYER_DOWN" | "LOWERLAYER_DOWN" => Self::LowerLayerDown,
            "NOTPRESENT" | "NOT_PRESENT" => Self::NotPresent,
            "TESTING" => Self::Testing,
            _ => Self::Other,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Up => "UP",
            Self::Down => "DOWN",
            Self::Unknown => "UNKNOWN",
            Self::Dormant => "DORMANT",
            Self::LowerLayerDown => "LOWERLAYERDOWN",
            Self::NotPresent => "NOTPRESENT",
            Self::Testing => "TESTING",
            Self::Other => "OTHER",
        }
    }

    pub fn is_up(self) -> bool {
        matches!(self, Self::Up)
    }
}

impl Serialize for OperState {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OperState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::parse_lossy(&value))
    }
}

#[cfg(all(unix, feature = "experimental-nl"))]
impl From<NetlinkOperState> for OperState {
    fn from(value: NetlinkOperState) -> Self {
        match value {
            NetlinkOperState::Up => Self::Up,
            NetlinkOperState::Down => Self::Down,
            NetlinkOperState::Unknown => Self::Unknown,
            NetlinkOperState::Dormant => Self::Dormant,
            NetlinkOperState::LowerLayerDown => Self::LowerLayerDown,
            NetlinkOperState::NotPresent => Self::NotPresent,
            NetlinkOperState::Testing => Self::Testing,
            _ => Self::Other,
        }
    }
}

pub async fn get(dev: Option<&str>) -> anyhow::Result<Vec<LinkOutput>> {
    get_with_backend(Backend::Json, dev).await
}

pub async fn get_with_backend(
    backend: Backend,
    dev: Option<&str>,
) -> anyhow::Result<Vec<LinkOutput>> {
    match backend {
        Backend::Json => json::get(dev).await,
        #[cfg(all(unix, feature = "experimental-nl"))]
        Backend::Netlink => nl::get(dev).await,
    }
}

#[cfg(test)]
mod tests {
    use super::{LinkOutput, OperState};

    #[test]
    fn operstate_parses_json_and_netlink_spellings() {
        assert_eq!(OperState::parse_lossy("UP"), OperState::Up);
        assert_eq!(OperState::parse_lossy("Up"), OperState::Up);
        assert_eq!(
            OperState::parse_lossy("LOWERLAYERDOWN"),
            OperState::LowerLayerDown
        );
        assert_eq!(
            OperState::parse_lossy("LowerLayerDown"),
            OperState::LowerLayerDown
        );
    }

    #[test]
    fn link_output_deserializes_typed_operstate() {
        let link: LinkOutput = serde_json::from_str(
            r#"{"ifindex":1,"ifname":"eth0","operstate":"UP","address":"aa:bb:cc:dd:ee:ff"}"#,
        )
        .expect("link json should deserialize");

        assert_eq!(link.operstate, Some(OperState::Up));
        assert_eq!(
            link.address.map(|mac| mac.to_string()).as_deref(),
            Some("aa:bb:cc:dd:ee:ff")
        );
    }
}
