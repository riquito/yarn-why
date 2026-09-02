#!/usr/bin/env bash
# Build the npm package: Rust -> wasm -> JS glue, then TypeScript -> dist/.
set -euo pipefail

cd "$(dirname "$0")"

# --out-dir is relative to the crate root, hence the npm/ prefix
(cd .. && wasm-pack build \
  --target web \
  --profile web-release \
  --out-dir npm/wasm \
  --out-name yarn_why)

# wasm-pack writes a package.json of its own; ours is the one that ships
rm -f wasm/package.json wasm/README.md wasm/LICENSE wasm/.gitignore

npm install --no-audit --no-fund
./node_modules/.bin/tsc --project tsconfig.json

echo "npm package built in $(pwd)/dist"
