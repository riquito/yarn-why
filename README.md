# yarn-why

`yarn why`, but fast

## Install

Download one of the [prebuilt binaries](https://github.com/riquito/yarn-why/releases)

or run

```
cargo install yarn-why
```

## Help

```
yarn-why 0.1.0-rc1

Usage:
    yarn-why [OPTIONS] package[@range] # read ./yarn.lock
    yarn-why [OPTIONS] package[@range] < /path/to/yarn.lock

Example:
    yarn-why lodash@^4.17.15

OPTIONS:
    -d, --max-depth [depth]  Truncate dependencies at that level [Default: 10]
    -D, --no-max-depth       Ignore max-depth (both default and user defined)
    -j, --json               Format the output as JSON
    -h, --help               Prints this help and exit
    -V, --version            Prints version information

ARGS:
    package[@range]          Package to search for, with or without range.
                             The range must match one in yarn.lock
```


## LICENSE

yarn-why is distributed under the GNU GPL license (version 3 or any later version).

See [LICENSE](./LICENSE) file for details.
