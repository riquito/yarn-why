use anyhow::Result;
use std::io::{BufRead, Read, Write};
use yarn_lock_parser::{parse_str, Entry};

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
    let mut stdin = std::io::BufReader::with_capacity(32 * 1024, stdin.lock());

    let stdout = std::io::stdout();
    let mut stdout = std::io::BufWriter::with_capacity(32 * 1024, stdout.lock());

    let mut yarnLockText: Vec<u8> = Vec::new();
    stdin.read_to_end(&mut yarnLockText)?;

    let entries = parse_str(std::str::from_utf8(&yarnLockText)?);
    println!("{:?}", &entries);

    stdout.flush()?;

    Ok(())
}
