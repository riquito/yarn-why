# yarn-why
[![version](https://img.shields.io/crates/v/yarn-why.svg)](https://crates.io/crates/yarn-why)
![ci](https://github.com/riquito/yarn-why/actions/workflows/ci.yml/badge.svg)
[![license](https://img.shields.io/crates/l/yarn-why.svg)](https://crates.io/crates/yarn-why)

`yarn why`, but fast

## Install

Download one of the [prebuilt binaries](https://github.com/riquito/yarn-why/releases)

or run

```
cargo install yarn-why
```

## Help

```
yarn-why 1.0.0-rc2

Usage:
    yarn-why [OPTIONS] package[@range] # read ./yarn.lock
    yarn-why [OPTIONS] package[@range] < /path/to/yarn.lock
    yarn-why [OPTIONS] package[@range] -y /path/to/yarn.lock

Example:
    yarn-why lodash@^4.17.15

OPTIONS:
    -d, --max-depth [depth]  Truncate dependencies at that level [Default: 10]
    -D, --no-max-depth       Ignore max-depth (both default and user defined)
        --(no)-dedup         Display packages at most once [Default: true]
    -j, --json               Format the output as JSON
    -h, --help               Prints this help and exit
    -V, --version            Prints version information
    -y, --yarn-lock-file     Path to a yarn.lock file to parse

ARGS:
    package[@range]          Package to search for, with or without range.
                             The range must match one in yarn.lock
```


## Benchmarks

Benchmarks run on Thinkpad T460s
- node 17.9.0
- yarn 1.22.18 / yarn 3.2.0
- using [renovate 35.45.5 yarn.lock file](https://github.com/renovatebot/renovate/blob/32.45.5/yarn.lock) (v1 first, then updating it)

(had to use -y because hyperfine would trigger stdin input)

```
$ hyperfine 'yarn-why -y yarn.lock lodash'
Benchmark #1: yarn-why -y yarn.lock lodash
  Time (mean ± σ):       5.2 ms ±   0.2 ms    [User: 4.2 ms, System: 1.0 ms]
  Range (min … max):     4.7 ms …   6.6 ms    417 runs

  Warning: Command took less than 5 ms to complete. Results might be inaccurate.

$ hyperfine 'yarn why lodash'
Benchmark #1: yarn why lodash
  Time (mean ± σ):      1.012 s ±  0.012 s    [User: 1.686 s, System: 0.101 s]
  Range (min … max):    0.994 s …  1.026 s    10 runs

# again, after updating yarn.lock using `yarn 3.2.0`

$ hyperfine 'yarn why lodash'
Benchmark #1: yarn why lodash
 ⠏ Current estimate: 45.455 s     █████████████████████████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ ETA 00:05:17
^C # was taking too long, I stopped it

hyperfine 'yarn-why -y yarn.lock lodash'
Benchmark #1: yarn-why -y yarn.lock lodash
  Time (mean ± σ):       6.2 ms ±   0.2 ms    [User: 5.2 ms, System: 1.0 ms]
  Range (min … max):     5.9 ms …   7.6 ms    351 runs
```

## LICENSE

yarn-why is distributed under the GNU GPL license (version 3 or any later version).

See [LICENSE](./LICENSE) file for details.
