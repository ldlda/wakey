// generated with ./scripts/map_static.py

macro_rules! hehe {
    // Folder
    (folder $name:ident { $($children:tt)* } $($rest:tt)*) => {
        pub mod $name {
            hehe!{$($children)*}
        }
        hehe!{$($rest)*}
    };
    // File
    (file $name:ident $file:literal $($rest:tt)*) => {
        pub const $name: &str = include_str!($file);
        hehe!{$($rest)*}
    };
    // Base case
    () => {};
}

hehe! {
    file HOME_2_HTML "../static/home_2.html"
    folder home_2 {
        file DOM_JS "../static/home_2/dom.js"
        file LEASES_JS "../static/home_2/leases.js"
        file MAIN_JS "../static/home_2/main.js"
        file STATUS_JS "../static/home_2/status.js"
        file STYLES_CSS "../static/home_2/styles.css"
        file UTILS_JS "../static/home_2/utils.js"
        file WAKE_JS "../static/home_2/wake.js"
    }
}
