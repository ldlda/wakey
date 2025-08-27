use std::collections::HashSet;

pub fn get_dev() -> HashSet<String> {
    let mut devs: HashSet<String> = HashSet::new();
    if let Ok(rd) = std::fs::read_dir("/sys/class/net") {
        for e in rd.flatten() {
            if let Ok(name) = e.file_name().into_string()
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

pub fn devs_sorted() -> Vec<String> {
    let mut v: Vec<String> = get_dev().into_iter().collect();
    v.sort();
    v
}

pub fn has_dev(name: &str) -> bool {
    get_dev().contains(name)
}
