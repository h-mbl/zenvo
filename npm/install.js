#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const https = require('https');
const { createWriteStream, existsSync, mkdirSync, chmodSync, unlinkSync } = require('fs');

const PACKAGE_VERSION = require('./package.json').version;
const GITHUB_REPO = 'h-mbl/zenvo';
const MAX_RETRIES = 3;
const RETRY_DELAY_MS = 1000;

// Platform mapping to GitHub release asset names
const PLATFORMS = {
  'darwin-x64': { zenvo: 'zenvo-darwin-x64', mcp: 'zenvo-darwin-x64-mcp' },
  'darwin-arm64': { zenvo: 'zenvo-darwin-arm64', mcp: 'zenvo-darwin-arm64-mcp' },
  'linux-x64': { zenvo: 'zenvo-linux-x64', mcp: 'zenvo-linux-x64-mcp' },
  'linux-arm64': { zenvo: 'zenvo-linux-arm64', mcp: 'zenvo-linux-arm64-mcp' },
  'win32-x64': { zenvo: 'zenvo-windows-x64.exe', mcp: 'zenvo-windows-x64-mcp.exe' },
};

function getPlatformKey() {
  return `${process.platform}-${process.arch}`;
}

function getPlatformBinaries() {
  const key = getPlatformKey();
  const binaries = PLATFORMS[key];

  if (!binaries) {
    console.error(`\n  Unsupported platform: ${key}`);
    console.error(`  Supported: ${Object.keys(PLATFORMS).join(', ')}\n`);
    console.error('  Alternative installation methods:');
    console.error('    cargo install zenvo');
    console.error('    brew install h-mbl/zenvo/zenvo\n');
    process.exit(1);
  }

  return binaries;
}

function getDownloadUrl(assetName) {
  return `https://github.com/${GITHUB_REPO}/releases/download/v${PACKAGE_VERSION}/${assetName}`;
}

function formatBytes(bytes) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

function progressBar(current, total, width = 30) {
  const percent = total > 0 ? current / total : 0;
  const filled = Math.round(width * percent);
  const empty = width - filled;
  const bar = '█'.repeat(filled) + '░'.repeat(empty);
  const percentStr = (percent * 100).toFixed(0).padStart(3);
  return `[${bar}] ${percentStr}% ${formatBytes(current)}/${formatBytes(total)}`;
}

function download(url, dest, redirectCount = 0) {
  return new Promise((resolve, reject) => {
    if (redirectCount > 5) {
      reject(new Error('Too many redirects'));
      return;
    }

    const request = https.get(url, (response) => {
      // Handle redirects (GitHub releases use them)
      if (response.statusCode === 302 || response.statusCode === 301) {
        download(response.headers.location, dest, redirectCount + 1)
          .then(resolve)
          .catch(reject);
        return;
      }

      if (response.statusCode === 404) {
        reject(new Error(`Release v${PACKAGE_VERSION} not found on GitHub`));
        return;
      }

      if (response.statusCode !== 200) {
        reject(new Error(`Download failed: HTTP ${response.statusCode}`));
        return;
      }

      const totalSize = parseInt(response.headers['content-length'], 10) || 0;
      let downloadedSize = 0;
      const file = createWriteStream(dest);

      // Progress reporting
      const showProgress = process.stdout.isTTY && totalSize > 0;

      response.on('data', (chunk) => {
        downloadedSize += chunk.length;
        if (showProgress) {
          process.stdout.write(`\r  ${progressBar(downloadedSize, totalSize)}`);
        }
      });

      response.pipe(file);

      file.on('finish', () => {
        file.close();
        if (showProgress) {
          process.stdout.write('\n');
        }
        resolve();
      });

      file.on('error', (err) => {
        unlinkSync(dest);
        reject(err);
      });
    });

    request.on('error', (err) => {
      reject(err);
    });

    request.setTimeout(30000, () => {
      request.destroy();
      reject(new Error('Download timeout'));
    });
  });
}

async function downloadWithRetry(url, dest) {
  let lastError;

  for (let attempt = 1; attempt <= MAX_RETRIES; attempt++) {
    try {
      await download(url, dest);
      return;
    } catch (error) {
      lastError = error;
      if (attempt < MAX_RETRIES) {
        console.log(`  Retry ${attempt}/${MAX_RETRIES - 1}...`);
        await new Promise(r => setTimeout(r, RETRY_DELAY_MS * attempt));
      }
    }
  }

  throw lastError;
}

async function downloadBinary(name, assetName, binDir, isWindows) {
  const localName = isWindows ? `${name}.exe` : name;
  const binaryPath = path.join(binDir, localName);

  // Check if binary already exists and is executable
  if (existsSync(binaryPath)) {
    try {
      const { execSync } = require('child_process');
      execSync(`"${binaryPath}" --version`, { stdio: 'ignore' });
      console.log(`  ${name} already installed, skipping`);
      return true;
    } catch {
      // Binary exists but doesn't work, re-download
      unlinkSync(binaryPath);
    }
  }

  const url = getDownloadUrl(assetName);
  console.log(`  Downloading ${name}...`);

  await downloadWithRetry(url, binaryPath);

  // Make executable on Unix
  if (!isWindows) {
    chmodSync(binaryPath, 0o755);
  }

  return true;
}

async function install() {
  const binDir = path.join(__dirname, 'bin');
  const isWindows = process.platform === 'win32';
  const binaries = getPlatformBinaries();

  // Create bin directory
  if (!existsSync(binDir)) {
    mkdirSync(binDir, { recursive: true });
  }

  console.log(`\n  Installing zenvo v${PACKAGE_VERSION}...`);
  console.log(`  Platform: ${getPlatformKey()}`);
  console.log(`  Downloading from GitHub Releases...\n`);

  try {
    // Download main CLI
    await downloadBinary('zenvo', binaries.zenvo, binDir, isWindows);

    // Download MCP server
    await downloadBinary('zenvo-mcp', binaries.mcp, binDir, isWindows);

    console.log('\n  ✓ zenvo installed successfully!');
    console.log('  Run "zenvo --help" to get started.');
    console.log('  MCP server available at "zenvo-mcp"\n');
  } catch (error) {
    console.error(`\n  ✗ Failed to install: ${error.message}\n`);
    console.error('  Alternative installation methods:');
    console.error('    cargo install zenvo');
    console.error('    brew install h-mbl/zenvo/zenvo');
    console.error(`    https://github.com/${GITHUB_REPO}/releases\n`);
    process.exit(1);
  }
}

// Only run if called directly (not required as module)
if (require.main === module) {
  install().catch((err) => {
    console.error('  Installation failed:', err.message);
    process.exit(1);
  });
}

module.exports = { install };
