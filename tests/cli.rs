use assert_cmd::Command;

const YARN_LOCK_V6_WITH_DEPS: &str = r#"# This file is generated by running "yarn install" inside your project.
# Manual changes might be lost - proceed with caution!

__metadata:
  version: 6
  cacheKey: 8

"foo@workspace:.":
  version: 0.0.0-use.local
  resolution: "foo@workspace:."
  dependencies:
    foolib: 1.2.3 || ^2.0.0
    buzz: "npm:^1.1.1"
  languageName: unknown
  linkType: soft

"foolib@npm:1.2.3 || ^2.0.0":
  version: 2.0.0
  resolution: "foolib@npm:2.0.0"
  checksum: 123061e52a0b3792c6a0472bf48ca6c337ccb58e92261049e7727a12c326b9627537e2ef8cb4453354d02c763b87c8b516f4eedfad99945c308927285bbc12ba
  languageName: node
  linkType: hard

"buzz@npm:^1.1.1":
  version: 1.1.2
  resolution: "buzz@npm:1.1.2"
  checksum: 58e92261049e7727a12c326b9627537e123061e52a0b3792c6a0472bf48ca6c337ccb2ef8cb4453354d02c763b87c8b516f4eedfad99945c308927285bbc12ba
  languageName: node
  linkType: hard
"#;

const YARN_LOCK_V6_ONLY_DIRECT_DEPS: &str = r#"# This file is generated by running "yarn install" inside your project.
# Manual changes might be lost - proceed with caution!

__metadata:
  version: 6
  cacheKey: 8

"foolib@npm:1.2.3 || ^2.0.0":
  version: 2.0.0
  resolution: "foolib@npm:2.0.0"
  checksum: 123061e52a0b3792c6a0472bf48ca6c337ccb58e92261049e7727a12c326b9627537e2ef8cb4453354d02c763b87c8b516f4eedfad99945c308927285bbc12ba
  languageName: node
  linkType: hard
"#;

// This is a modified version where I removed attributes
// to not clutter too mouch the test (need to recreate it
// once we move this kind of definition into a fixtures directory)
const YARN_LOCK_V8_WITH_PATCH_PROTOCOL: &str = r#"# This file is generated by running "yarn install" inside your project.
# Manual changes might be lost - proceed with caution!

__metadata:
  version: 8
  cacheKey: 10c0

"foobar@workspace:.":
  version: 0.0.0-use.local
  resolution: "foobar@workspace:."
  dependencies:
    vite: "npm:^5.2.0"
  languageName: unknown
  linkType: soft

"fsevents@npm:~2.3.2, fsevents@npm:~2.3.3":
  version: 2.3.3
  resolution: "fsevents@npm:2.3.3"
  dependencies:
    node-gyp: "npm:latest"
  checksum: 10c0/a1f0c44595123ed717febbc478aa952e47adfc28e2092be66b8ab1635147254ca6cfe1df792a8997f22716d4cbafc73309899ff7bfac2ac3ad8cf2e4ecc3ec60
  conditions: os=darwin
  languageName: node
  linkType: hard

"fsevents@patch:fsevents@npm%3A~2.3.2#optional!builtin<compat/fsevents>, fsevents@patch:fsevents@npm%3A~2.3.3#optional!builtin<compat/fsevents>":
  version: 2.3.3
  resolution: "fsevents@patch:fsevents@npm%3A2.3.3#optional!builtin<compat/fsevents>::version=2.3.3&hash=df0bf1"
  dependencies:
    node-gyp: "npm:latest"
  conditions: os=darwin
  languageName: node
  linkType: hard

"node-gyp@npm:latest":
  version: 10.0.1
  resolution: "node-gyp@npm:10.0.1"
  checksum: 10c0/abddfff7d873312e4ed4a5fb75ce893a5c4fb69e7fcb1dfa71c28a6b92a7f1ef6b62790dffb39181b5a82728ba8f2f32d229cf8cbe66769fe02cea7db4a555aa
  languageName: node
  linkType: hard

"rollup@npm:^4.13.0":
  version: 4.13.0
  resolution: "rollup@npm:4.13.0"
  dependencies:
    fsevents: "npm:~2.3.2"
  checksum: 10c0/90f8cdf9c2115223cbcfe91d932170a85c0928ae1943f45af6877907ea150585b80f656cf2bc471c6f809cb7e158dd85dbea9f91ab4fd5bce0eaf6c3f5f4fd92
  languageName: node
  linkType: hard

"vite@npm:^5.2.0":
  version: 5.2.4
  resolution: "vite@npm:5.2.4"
  dependencies:
    fsevents: "npm:~2.3.3"
    rollup: "npm:^4.13.0"
  dependenciesMeta:
    fsevents:
      optional: true
  checksum: 10c0/a8a57da83b5a46d9fc135fc4c51d08e1e60cdc4263ce6e7b23d60501c70605dd541726c331ab4d837ad774cdbf0b78bac088da770287620f81ad8b4d7b39dd74
  languageName: node
  linkType: hard
"#;

#[test]
fn it_fails_and_display_the_help_when_invoked_without_args() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let assert = cmd.assert().failure().code(1);
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert!(stdout.contains("Usage:"), "Output was: {stdout}");
}

#[test]
fn it_succeed_and_display_the_help_when_invoked_with_help_option() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let assert = cmd.args(["-h"]).assert().success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert!(stdout.contains("Usage:"), "Output was: {stdout}");
}

#[test]
fn it_displays_the_version() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let assert = cmd.args(["-V"]).assert().success();
    let stdout = std::str::from_utf8(&assert.get_output().stdout).unwrap();
    assert_eq!(
        concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"), "\n"),
        stdout
    );
}

#[test]
fn it_finds_a_package_with_range() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let assert = cmd
        .args(["foolib", ">=1.2.3, <3"])
        .write_stdin(YARN_LOCK_V6_WITH_DEPS)
        .assert();

    assert.success().stdout(
        r#"└─ foolib@2.0.0 (via 1.2.3 || ^2.0.0)
"#,
    );
}

#[test]
fn it_finds_a_package_without_range() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let assert = cmd
        .args(["foolib"])
        .write_stdin(YARN_LOCK_V6_WITH_DEPS)
        .assert();

    assert.success().stdout(
        r#"└─ foolib@2.0.0 (via 1.2.3 || ^2.0.0)
"#,
    );
}

#[test]
fn it_finds_a_package_whose_dep_is_using_npm_protocol() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let assert = cmd
        .args(["buzz"])
        .write_stdin(YARN_LOCK_V6_WITH_DEPS)
        .assert();

    assert.success().stdout(
        r#"└─ buzz@1.1.2 (via ^1.1.1)
"#,
    );
}

#[test]
fn it_finds_a_package_in_a_yarn_lock_with_only_direct_deps() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let assert = cmd
        .args(["foolib"])
        .write_stdin(YARN_LOCK_V6_ONLY_DIRECT_DEPS)
        .assert();

    assert
        .success()
        .stdout("└─ foolib@2.0.0 (via 1.2.3 || ^2.0.0)\n");
}

#[test]
fn it_exit_with_error_if_the_package_cannot_be_found() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let assert = cmd
        .args(["not-there"])
        .write_stdin(YARN_LOCK_V6_ONLY_DIRECT_DEPS)
        .assert();

    assert.failure().stdout("Package not found\n").code(1);
}

#[test]
fn it_ignores_entries_with_the_patch_protocol() {
    let mut cmd = Command::cargo_bin(env!("CARGO_PKG_NAME")).unwrap();

    let assert = cmd
        .args(["node-gyp"])
        .write_stdin(YARN_LOCK_V8_WITH_PATCH_PROTOCOL)
        .assert();

    assert.success().stdout(
        r#"└─ vite@5.2.4 (via ^5.2.0)
   ├─ fsevents@2.3.3 (via ~2.3.3)
   │  └─ node-gyp@10.0.1 (via latest)
   └─ rollup@4.13.0 (via ^4.13.0)
      └─ fsevents@2.3.3 (via ~2.3.2)
         └─ node-gyp@10.0.1 (via latest)
"#,
    );
}
