const fs = require("fs");
const path = require("path");
const https = require("https");
const { execSync } = require("child_process");
const os = require("os");

const VERSION = require("./package.json").version;
const REPO = "Meru143/argus";

const PLATFORM_MAP = {
  "darwin-x64": "argus-x86_64-apple-darwin.tar.gz",
  "darwin-arm64": "argus-aarch64-apple-darwin.tar.gz",
  "linux-x64": "argus-x86_64-unknown-linux-gnu.tar.gz",
  "linux-arm64": "argus-aarch64-unknown-linux-gnu.tar.gz",
  "win32-x64": "argus-x86_64-pc-windows-msvc.zip",
};

function getPlatformKey() {
  return `${os.platform()}-${os.arch()}`;
}

function getDownloadUrl(asset) {
  return `https://github.com/${REPO}/releases/download/v${VERSION}/${asset}`;
}

function download(url) {
  return new Promise((resolve, reject) => {
    const follow = (url, redirects = 0) => {
      if (redirects > 5) return reject(new Error("Too many redirects"));

      const mod = url.startsWith("https") ? https : require("http");
      mod
        .get(url, { headers: { "User-Agent": "argus-ai-npm" } }, (res) => {
          if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
            return follow(res.headers.location, redirects + 1);
          }
          if (res.statusCode !== 200) {
            return reject(new Error(`Download failed: HTTP ${res.statusCode}`));
          }

          const chunks = [];
          res.on("data", (chunk) => chunks.push(chunk));
          res.on("end", () => resolve(Buffer.concat(chunks)));
          res.on("error", reject);
        })
        .on("error", reject);
    };
    follow(url);
  });
}

async function install() {
  const key = getPlatformKey();
  const asset = PLATFORM_MAP[key];

  if (!asset) {
    console.error(`Unsupported platform: ${key}`);
    console.error(`Supported: ${Object.keys(PLATFORM_MAP).join(", ")}`);
    console.error("You can build from source: cargo install --git https://github.com/Meru143/argus");
    process.exit(1);
  }

  const url = getDownloadUrl(asset);
  const binDir = path.join(__dirname, "bin");
  const isWindows = os.platform() === "win32";
  const binaryName = isWindows ? "argus-bin.exe" : "argus-bin";
  const binaryPath = path.join(binDir, binaryName);

  // Skip if already installed
  if (fs.existsSync(binaryPath)) {
    return;
  }

  console.log(`Downloading argus v${VERSION} for ${key}...`);

  try {
    const data = await download(url);
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "argus-"));
    const archivePath = path.join(tmpDir, asset);

    fs.writeFileSync(archivePath, data);
    fs.mkdirSync(binDir, { recursive: true });

    if (asset.endsWith(".zip")) {
      // Windows: use PowerShell to extract
      execSync(
        `powershell -command "Expand-Archive -Path '${archivePath}' -DestinationPath '${tmpDir}'"`,
        { stdio: "pipe" }
      );
    } else {
      // Unix: use tar
      execSync(`tar xzf "${archivePath}" -C "${tmpDir}"`, { stdio: "pipe" });
    }

    // Find the binary in extracted files
    const extracted = findBinary(tmpDir, isWindows ? "argus.exe" : "argus");
    if (!extracted) {
      throw new Error("Could not find argus binary in archive");
    }

    fs.copyFileSync(extracted, binaryPath);
    if (!isWindows) {
      fs.chmodSync(binaryPath, 0o755);
    }

    // Cleanup
    fs.rmSync(tmpDir, { recursive: true, force: true });

    console.log(`Installed argus v${VERSION} to ${binaryPath}`);
  } catch (err) {
    console.error(`Failed to install argus: ${err.message}`);
    console.error("You can build from source: cargo install --git https://github.com/Meru143/argus");
    process.exit(1);
  }
}

function findBinary(dir, name) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      const found = findBinary(full, name);
      if (found) return found;
    } else if (entry.name === name) {
      return full;
    }
  }
  return null;
}

install();
