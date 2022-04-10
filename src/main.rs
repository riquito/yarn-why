use anyhow::Result;
use std::io::{BufRead, Write};

const HELP: &str = concat!(
    "yarn-why ",
    env!("CARGO_PKG_VERSION"),
    "
Find why you have a dependency, fast

USAGE:
    yarn-why [FLAGS] [OPTIONS] < yarn.lock

LICENSE: GPL-3.0-or-later
"
);

fn main() -> Result<()> {
    let stdin = std::io::stdin();
    let stdin = std::io::BufReader::with_capacity(32 * 1024, stdin.lock());

    let stdout = std::io::stdout();
    let mut stdout = std::io::BufWriter::with_capacity(32 * 1024, stdout.lock());

    stdin
        .lines()
        .try_for_each::<_, Result<()>>(|maybe_line| -> Result<()> {
            stdout.write_all(maybe_line?.as_bytes())?;
            stdout.write_all(&[b'\n'])?;
            Ok(())
        })?;

    stdout.flush()?;

    Ok(())
}
