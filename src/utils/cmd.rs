use std::io;

pub(crate) async fn exec_command<S: AsRef<std::ffi::OsStr>>(
    cmd: S,
    args: impl IntoIterator<Item = S>,
) -> io::Result<std::process::Output> {
    let mut u = tokio::process::Command::new(cmd);
    u.args(args);
    u.output().await
}