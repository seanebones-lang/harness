#!/bin/bash
# Offline build support for submissions
set -e
cargo vendor vendor/
echo '[source.crates-io]' > .cargo/config.toml
echo 'replace-with = "vendored-sources"' >> .cargo/config.toml
echo '' >> .cargo/config.toml
echo '[source.vendored-sources]' >> .cargo/config.toml
echo 'directory = "vendor"' >> .cargo/config.toml
echo "Vendor directory created. Use: cargo build --offline"