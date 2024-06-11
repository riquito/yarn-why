use serde::{Deserialize, Serialize};
use yarn_lock_parser::Entry;

#[derive(Serialize, Deserialize)]
pub struct Records<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub descriptor: &'a str,
}

pub fn iter_flat_dependencies<'a>(entries: &'a [Entry]) -> impl Iterator<Item = Records<'a>> {
    entries.iter().flat_map(|e| {
        e.descriptors.iter().map(|d| Records {
            name: e.name,
            version: e.version,
            descriptor: d.1,
        })
    })
}
