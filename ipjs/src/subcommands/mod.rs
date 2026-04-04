pub mod address;
pub mod neighbor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Json,
    #[cfg(all(unix, feature = "experimental-nl"))]
    Netlink,
}
