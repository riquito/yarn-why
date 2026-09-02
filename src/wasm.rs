//! JavaScript bindings, compiled only for `wasm32-unknown-unknown`.
//!
//! Options come in as a JSON string and results go out as JSON strings: the
//! ergonomics (defaults, camelCase, types) live in the TypeScript wrapper
//! under `npm/`, which is a better place for them than here.

use serde::Deserialize;
use wasm_bindgen::prelude::*;

use crate::{Format, Options, Report, MAX_DEPTH_DEFAULT, MAX_PKG_VISITS_DEFAULT};

/// Mirrors `WhyOptions` in npm/src/index.ts.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
struct JsOptions {
    max_depth: Option<usize>,
    no_max_depth: bool,
    dedup: bool,
    full_tree: bool,
    range: Option<String>,
    max_pkg_visits: usize,
}

impl Default for JsOptions {
    fn default() -> Self {
        Self {
            max_depth: Some(MAX_DEPTH_DEFAULT),
            no_max_depth: false,
            dedup: true,
            full_tree: false,
            range: None,
            max_pkg_visits: MAX_PKG_VISITS_DEFAULT,
        }
    }
}

impl JsOptions {
    fn into_options(self) -> Result<Options, JsError> {
        let range = self
            .range
            .as_deref()
            .map(semver::VersionReq::parse)
            .transpose()
            .map_err(|e| JsError::new(&format!("invalid range: {e}")))?;

        Ok(Options {
            max_depth: if self.no_max_depth {
                None
            } else {
                self.max_depth
            },
            dedup: self.dedup,
            full_tree: self.full_tree,
            range,
            // ANSI escapes would only get in the way of a JS caller
            color: false,
            max_pkg_visits: self.max_pkg_visits,
        })
    }
}

fn parse_options(options_json: &str) -> Result<Options, JsError> {
    let js_opts: JsOptions = serde_json::from_str(options_json)
        .map_err(|e| JsError::new(&format!("invalid options: {e}")))?;
    js_opts.into_options()
}

fn run(
    lockfile: &str,
    query: &str,
    options_json: &str,
    format: Format,
) -> Result<Option<String>, JsError> {
    let opts = parse_options(options_json)?;

    match crate::why(lockfile, query, &opts, format).map_err(|e| JsError::new(&e.to_string()))? {
        Report::Found(output) => Ok(Some(output)),
        Report::NotFound => Ok(None),
    }
}

/// The dependency paths as a JSON array, or `undefined` when not found.
#[wasm_bindgen(js_name = whyJson)]
pub fn why_json(
    lockfile: &str,
    query: &str,
    options_json: &str,
) -> Result<Option<String>, JsError> {
    run(lockfile, query, options_json, Format::Json)
}

/// The same tree the CLI prints, or `undefined` when not found.
#[wasm_bindgen(js_name = whyText)]
pub fn why_text(
    lockfile: &str,
    query: &str,
    options_json: &str,
) -> Result<Option<String>, JsError> {
    run(lockfile, query, options_json, Format::Text)
}

/// Every (name, version, descriptor) in the lockfile, as a JSON array.
///
/// The CLI's `--print-records` emits the same records as JSONL.
#[wasm_bindgen(js_name = records)]
pub fn records(lockfile: &str) -> Result<String, JsError> {
    // no query and no range, so nothing gets filtered out
    let entries =
        crate::parse_lockfile(lockfile, "", None).map_err(|e| JsError::new(&e.to_string()))?;

    let records: Vec<_> = crate::records::iter_flat_dependencies(&entries).collect();

    serde_json::to_string(&records).map_err(|e| JsError::new(&e.to_string()))
}

/// Better panic messages in the console. Called by the wrapper on load.
#[wasm_bindgen(js_name = setPanicHook)]
pub fn set_panic_hook() {
    console_error_panic_hook::set_once();
}
