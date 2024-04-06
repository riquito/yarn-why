# Changelog

## [Unreleased]

## [1.0.0-rc3] - 2022-08-01

### Added
- display the output as an ASCII tree
- new option `--filter`, to limit which versions to search for
- new ption `--dedup` to remove duplicate results
- colorize output when using a TTY
- env var MAX_PKG_VISITS can be set to bypass hypotetical infinite loops.
  It stops searching children of a package when it was visited already more than
  MAX_PKG_VISITS times
- fixed a bug detecting dependencies on newer versions of yarn.lock
- fix duplication caused by dependencies using patch protocol

## [1.0.0-rc2] - 2022-05-10

### Added
- add option -y to to read yarn.lock from a given path
### Doc
- add some benchmarks
- better help output
### CI
- automatically merge dependencies that pass tests
### Chore
- updated dependencies

## [1.0.0-rc1] - 2022-05-02
- first release