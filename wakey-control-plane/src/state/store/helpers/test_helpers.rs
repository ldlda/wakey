use std::fs;
use std::time::Duration;

use super::super::Store;

#[cfg(test)]
pub struct TestStore {
    store: Option<Store>,
    pub dir: std::path::PathBuf,
}

#[cfg(test)]
impl TestStore {
    pub fn store(&self) -> &Store {
        self.store.as_ref().expect("store already taken")
    }

    pub async fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("wakey-cp-test-{}", uuid::Uuid::new_v4()));
        let db_path = dir.join("state.sqlite3");
        let store = Store::load_or_init(&db_path, Vec::new(), Duration::from_secs(60))
            .await
            .expect("store should initialize");
        TestStore {
            store: Some(store),
            dir,
        }
    }
}

#[cfg(test)]
impl Drop for TestStore {
    fn drop(&mut self) {
        self.store.take();
        let _ = fs::remove_dir_all(&self.dir);
    }
}
