use serde::Serialize;
use serde_with::skip_serializing_none;

use crate::model::DeviceFilters;

#[skip_serializing_none]
#[derive(Debug, Default, Serialize)]
pub struct Status<T> {
    pub name: Option<String>,
    pub table: Vec<T>,
    pub filters: DeviceFilters,
}
