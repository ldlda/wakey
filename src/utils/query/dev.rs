use lda_ipjs::subcommands::address::json as ipjs_json;
use std::collections::HashSet;

pub async fn get_dev() -> HashSet<String> {
    let mut devs: HashSet<String> = HashSet::new();
    if let Ok(items) = ipjs_json::get(None).await {
        for item in items {
            if item.ifname != "lo" && !item.ifname.is_empty() {
                devs.insert(item.ifname);
            }
        }
    }
    devs
}

pub async fn devs_sorted() -> Vec<String> {
    let mut v: Vec<String> = get_dev().await.into_iter().collect();
    v.sort();
    v
}

pub async fn has_dev(name: &str) -> bool {
    get_dev().await.contains(name)
}
