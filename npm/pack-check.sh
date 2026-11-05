#!/usr/bin/env bash
# Pack every npm package, then run the launcher against the local release binary.
# Usage: from anywhere, after `cargo build --release --locked`.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

os="$(uname -s)"
mach="$(uname -m)"
ext=""
case "$os" in
  Linux)
    case "$mach" in
      x86_64) id=linux-x64 ;;
      aarch64|arm64) id=linux-arm64 ;;
      *) echo "unsupported linux $mach"; exit 1 ;;
    esac
    ;;
  Darwin)
    case "$mach" in
      x86_64) id=darwin-x64 ;;
      arm64) id=darwin-arm64 ;;
      *) echo "unsupported darwin $mach"; exit 1 ;;
    esac
    ;;
  MINGW*|MSYS*|CYGWIN*)
    id=windows-x64
    ext=".exe"
    ;;
  *)
    echo "unsupported $os $mach"
    exit 1
    ;;
esac

ver="$(grep -m 1 '^version = ' Cargo.toml | cut -d '"' -f 2)"
export VER="$ver"

node -e '
const fs = require("fs");
const server = JSON.parse(fs.readFileSync("server.json", "utf8"));
const pkg = JSON.parse(fs.readFileSync("npm/relatum/package.json", "utf8"));
if (server.$schema !== "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json") {
  throw new Error("server.json schema");
}
if (server.description.length < 1 || server.description.length > 100) {
  throw new Error("description length " + server.description.length);
}
if (server.name !== "io.github.IronAdamant/relatum") throw new Error("server name");
if (pkg.mcpName !== server.name) throw new Error("mcpName");
if (pkg.dependencies && Object.keys(pkg.dependencies).length) throw new Error("dependencies");
const optional = Object.keys(pkg.optionalDependencies || {}).sort();
const expect = [
  "@iron_adamant/relatum-darwin-arm64",
  "@iron_adamant/relatum-darwin-x64",
  "@iron_adamant/relatum-linux-arm64",
  "@iron_adamant/relatum-linux-x64",
  "@iron_adamant/relatum-windows-x64",
];
if (optional.join("\n") !== expect.join("\n")) throw new Error("optionalDependencies");
const entry = server.packages && server.packages[0];
if (!entry || entry.registryType !== "npm") throw new Error("registryType");
if (entry.identifier !== "@iron_adamant/relatum") throw new Error("identifier");
if (!entry.transport || entry.transport.type !== "stdio") throw new Error("transport");
if (!entry.packageArguments || entry.packageArguments[0].value !== "mcp") throw new Error("mcp arg");
const ver = process.env.VER;
if (entry.version !== ver || pkg.version !== ver || server.version !== ver) {
  throw new Error("version");
}
'

stage="$(mktemp -d)"
out="$(mktemp -d)"
cleanup() { rm -rf "$stage" "$out"; }
trap cleanup EXIT

cp -a "$root/npm/." "$stage/"
pkg="relatum-${id}"
for stem in relatum relatum-mcp swarmnotes; do
  src="target/release/${stem}${ext}"
  if [ ! -f "$src" ]; then
    echo "missing $src. Run: cargo build --release --locked"
    exit 1
  fi
  cp "$src" "$stage/$pkg/${stem}${ext}"
  if [ -z "$ext" ]; then
    chmod +x "$stage/$pkg/${stem}${ext}"
  fi
done

shopt -s nullglob
dirs=("$stage"/relatum "$stage"/relatum-*)
if [ "${#dirs[@]}" -ne 6 ]; then
  echo "expected 6 npm packages, found ${#dirs[@]}"
  exit 1
fi
for dir in "${dirs[@]}"; do
  npm pack --quiet --pack-destination "$out" "$dir" >/dev/null
done

main_tgz="$out/iron_adamant-relatum-${ver}.tgz"
plat_tgz="$out/iron_adamant-${pkg}-${ver}.tgz"
test -f "$main_tgz"
test -f "$plat_tgz"
main_list="$(tar -tzf "$main_tgz")"
plat_list="$(tar -tzf "$plat_tgz")"
[[ "$main_list" == *"package/bin/relatum.js"* ]]
[[ "$plat_list" == *"package/relatum${ext}"* ]]

work="$(mktemp -d)"
mkdir -p "$work/node_modules/@iron_adamant"
tar -xzf "$main_tgz" -C "$work/node_modules/@iron_adamant"
mv "$work/node_modules/@iron_adamant/package" "$work/node_modules/@iron_adamant/relatum"
tar -xzf "$plat_tgz" -C "$work/node_modules/@iron_adamant"
mv "$work/node_modules/@iron_adamant/package" "$work/node_modules/@iron_adamant/$pkg"

launcher="$work/node_modules/@iron_adamant/relatum/bin/relatum.js"
help="$(node "$launcher" --help)"
[[ "$help" == *"Pass-by-reference notes for agent swarms."* ]]

req='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"ci","version":"0"}}}'
resp="$(printf '%s\n' "$req" | node "$launcher" mcp)"
[[ "$resp" == *'"name":"relatum"'* ]]
[[ "$resp" == *"\"version\":\"${ver}\""* ]]

mcp_launcher="$work/node_modules/@iron_adamant/relatum/bin/relatum-mcp.js"
resp="$(printf '%s\n' "$req" | node "$mcp_launcher")"
[[ "$resp" == *'"name":"relatum"'* ]]

echo "npm pack ok ($pkg)"
rm -rf "$work"
