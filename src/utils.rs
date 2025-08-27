// pub const LDA_MACS: [[u8; 6]; 2] = [
//     // ether
//     [0x04, 0x7c, 0x16, 0x79, 0x6d, 0xee],
//     // wifi
//     [0xbc, 0x09, 0x1b, 0xec, 0x65, 0xd0],
// ];

/// is it time to lookup host lda.lan for this...
// pub static LDA_MACS_2: LazyLock<[MacAddr; 2]> = LazyLock::new(|| LDA_MACS.map(MacAddr::from));
pub mod wake;

use crate::utils::query::_get_macs_2_1;

pub mod cmd;
pub mod error;
/// generic so you can do "123.45.67.89:22" or "lda.lan:22" as an input
// this is so bad
pub mod ping;
pub mod query;
pub mod route;

// no custom ip deserializer needed when using axum_extra::extract::Query
// but we add a generic one to ignore blanks and accept OneOrMany
pub(crate) mod parse;

pub async fn _status_build(machine_name: &str) -> String {
    let formatted_macs = match _get_macs_2_1(machine_name).await {
        Ok(table) => {
            let the: String = table
                .iter()
                .map(|(ip, mac, state)| {
                    let mac_str = // if let Some(mac) = mac {
                        mac.to_string()
                    // } else {
                    //     "None".into()
                    // }
                    ;
                    format!(
                        "<tr><td>{ip}</td><td>{mac_str}</td><td>{state}</td></tr>",
                        state = state._dumber_state()
                    )
                })
                .collect();
            format!(
                r#"<p>info of {machine_name}:</p>
<table>
<tr><th>IP</th><th>MAC</th><th>State</th></tr>
{the}
</table>"#
            )
        }
        Err(e) => format!("<p>errors getting table for {machine_name}: {e}</p>"),
    };

    format!(
        r#"
<html>
<body>
{formatted_macs}
</body>
</html>     
"#,
    )
}
