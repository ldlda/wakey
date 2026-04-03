pub use wakey_core::QueryInput as QueryType;

pub async fn parse_query(q: String) -> QueryType {
    wakey_linux::devices::classify_query(q).await
}
