//! Tests what happens when a client attempts to connect to a local socket that doesn't exist.

use super::util::*;
use color_eyre::eyre::*;
use interprocess::local_socket::tokio::LocalSocketStream;
use std::io;

pub async fn run_and_verify_error(prefer_namespaced: bool) -> TestResult {
    use io::ErrorKind::*;
    let err = match client(prefer_namespaced).await {
        Err(e) => e.downcast::<io::Error>()?,
        Ok(()) => bail!("client successfully connected to nonexistent server"),
    };
    ensure!(
        matches!(err.kind(), NotFound | ConnectionRefused),
        "expected error to be 'not found' or 'connection refused', received '{}'",
        err
    );
    Ok(())
}
async fn client(prefer_namespaced: bool) -> TestResult {
    let nm = NameGen::new_auto(make_id!(), prefer_namespaced).next().unwrap();
    LocalSocketStream::connect(&*nm).await.context("connect failed")?;
    Ok(())
}
