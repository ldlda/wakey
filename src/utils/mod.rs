// pub const LDA_MACS: [[u8; 6]; 2] = [
//     // ether
//     [0x04, 0x7c, 0x16, 0x79, 0x6d, 0xee],
//     // wifi
//     [0xbc, 0x09, 0x1b, 0xec, 0x65, 0xd0],
// ];

// is it time to lookup host lda.lan for this...
// pub static LDA_MACS_2: LazyLock<[MacAddr; 2]> = LazyLock::new(|| LDA_MACS.map(MacAddr::from));

/// generic so you can do "123.45.67.89:22" or "lda.lan:22" as an input
// this is so bad
pub mod ping;
pub mod query;
