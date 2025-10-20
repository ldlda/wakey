use super::{WakeStatus, WakeTarget, WakeTargetResult};
use crate::route::wake::{
    WakeTarget as RouteWakeTarget, WakeTargetResult as RouteWakeResult,
    WakeTargetStatus as RouteWakeStatus,
};

#[derive(Debug, Clone, Copy)]
pub struct Incomplete;

impl TryFrom<RouteWakeTarget> for WakeTarget {
    type Error = Incomplete;
    fn try_from(value: RouteWakeTarget) -> Result<Self, Self::Error> {
        if let RouteWakeTarget {
            ip: Some(ip),
            mac: Some(mac),
        } = value
        {
            Ok(Self { ip, mac })
        } else {
            Err(Incomplete)
        }
    }
}

impl From<WakeTarget> for RouteWakeTarget {
    fn from(WakeTarget { ip, mac }: WakeTarget) -> Self {
        Self {
            ip: Some(ip),
            mac: Some(mac),
        }
    }
}

impl From<WakeTargetResult> for RouteWakeResult {
    fn from(WakeTargetResult { target, status }: WakeTargetResult) -> Self {
        Self {
            target: target.into(),
            status: status.into(),
        }
    }
}

impl RouteWakeTarget {
    pub fn to_incomplete(self) -> RouteWakeResult {
        RouteWakeResult {
            target: self,
            status: RouteWakeStatus::Incomplete,
        }
    }
    pub fn is_incomplete(&self) -> bool {
        !matches!(
            self,
            Self {
                ip: Some(_),
                mac: Some(_)
            }
        )
    }
}

impl From<WakeStatus> for RouteWakeStatus {
    fn from(value: WakeStatus) -> Self {
        match value {
            WakeStatus::NonexistentAddress => Self::NonexistentAddress,
            WakeStatus::Success => Self::Succeed,
            WakeStatus::WrongSize => Self::WrongSize,
        }
    }
}
