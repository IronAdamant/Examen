"use strict";

const { spawn } = require("child_process");
const path = require("path");

const names = {
  "linux-x64": "@iron_adamant/relatum-linux-x64",
  "linux-arm64": "@iron_adamant/relatum-linux-arm64",
  "darwin-x64": "@iron_adamant/relatum-darwin-x64",
  "darwin-arm64": "@iron_adamant/relatum-darwin-arm64",
  "win32-x64": "@iron_adamant/relatum-windows-x64",
};

function targetPackage() {
  return names[process.platform + "-" + process.arch] || "";
}

function binaryPath(stem) {
  const pkg = targetPackage();
  const file = process.platform === "win32" ? stem + ".exe" : stem;
  if (!pkg) {
    console.error("No Relatum binary is published for " + process.platform + " " + process.arch);
    process.exit(1);
  }
  let root;
  try {
    root = path.dirname(require.resolve(pkg + "/package.json"));
  } catch (_err) {
    console.error(
      "The platform package " + pkg + " is not installed. " +
      "Install Relatum from the GitHub Release, or reinstall so the optional dependency for this OS is selected."
    );
    process.exit(1);
  }
  return path.join(root, file);
}

function launch(stem) {
  const child = spawn(binaryPath(stem), process.argv.slice(2), { stdio: "inherit" });
  child.on("error", (err) => {
    console.error("Failed to start " + stem + ": " + err.message);
    process.exit(1);
  });
  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code == null ? 1 : code);
  });
}

module.exports = launch;
