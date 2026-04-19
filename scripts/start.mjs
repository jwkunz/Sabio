import { existsSync } from "node:fs";
import { spawn } from "node:child_process";
import { join } from "node:path";
import { platform } from "node:os";

const isWindows = platform() === "win32";
const binaryName = isWindows ? "sabio-server.exe" : "sabio-server";
const localBinary = join(process.cwd(), binaryName);

const command = existsSync(localBinary) ? localBinary : "cargo";
const args = existsSync(localBinary)
  ? []
  : ["run", "--manifest-path", "server/Cargo.toml", "--release"];

const child = spawn(command, args, {
  stdio: "inherit",
  shell: isWindows
});

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 0);
});
