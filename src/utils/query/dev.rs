use std::collections::HashSet;
// /// 50ms
// pub async fn get_dev() -> HashSet<String> {
//     use lda_ipjs::subcommands::address::json as ipjs_json;
//     let mut devs: HashSet<String> = HashSet::new();
//     if let Ok(items) = ipjs_json::get(None).await {
//         for item in items {
//             if item.ifname != "lo" && !item.ifname.is_empty() {
//                 devs.insert(item.ifname);
//             }
//         }
//     }
//     devs
// }

/// 3ms
pub async fn get_dev() -> HashSet<String> {
    use std::fs;
    fn get_dev() -> HashSet<String> {
        let mut devs: HashSet<String> = HashSet::new();
        if let Ok(rd) = std::fs::read_dir("/sys/class/net") {
            for e in rd.flatten() {
                if e.file_type()
                    .map(|ft| {
                        if ft.is_symlink() {
                            // true
                            fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false)
                        } else {
                            ft.is_dir()
                        }
                    })
                    .unwrap_or(false)
                    && let Ok(name) = e.file_name().into_string()
                    && name != "lo"
                    && !name.is_empty()
                {
                    devs.insert(name);
                }
            }
        } else if let Ok(txt) = std::fs::read_to_string("/proc/net/dev") {
            for line in txt.lines().skip(2) {
                if let Some((name, _rest)) = line.split_once(':') {
                    let n = name.trim().to_string();
                    if n != "lo" && !n.is_empty() {
                        devs.insert(n);
                    }
                }
            }
        }
        devs
    }
    tokio::task::spawn_blocking(get_dev)
        .await
        .unwrap_or_default()
}

pub async fn devs_sorted() -> Vec<String> {
    let mut v: Vec<String> = get_dev().await.into_iter().collect();
    v.sort();
    v
}

pub async fn has_dev(name: &str) -> bool {
    get_dev().await.contains(name)
}
