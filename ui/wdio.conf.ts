import { resolve } from "node:path";

const application =
  resolve(import.meta.dirname, "../target/release/chess") +
  (process.platform === "win32" ? ".exe" : "");
const service = {
  appBinaryPath: application,
  driverProvider: "embedded",
  autoInstallTauriDriver: false,
  autoDownloadEdgeDriver: false,
};

export const config = {
  tsConfigPath: "./tsconfig.runtime.json",
  runner: "local",
  specs: ["./tests/native/**/*.e2e.ts"],
  maxInstances: 1,
  services: [["@wdio/tauri-service", service]],
  capabilities: [{ browserName: "tauri", "tauri:options": { application } }],
  framework: "mocha",
  reporters: ["spec"],
  logLevel: "warn",
  connectionRetryCount: 0,
  waitforTimeout: 10000,
  mochaOpts: { timeout: 30000 },
};
