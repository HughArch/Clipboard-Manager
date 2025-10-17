#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

// 颜色输出函数
const colors = {
  red: '\x1b[31m',
  green: '\x1b[32m',
  yellow: '\x1b[33m',
  blue: '\x1b[34m',
  reset: '\x1b[0m'
};

function log(color, message) {
  console.log(`${colors[color]}${message}${colors.reset}`);
}

try {
  log('blue', '🔄 开始同步版本信息...');

  // 1. 从 version.ts 读取版本号
  log('yellow', '📖 读取前端版本配置...');
  const versionTsPath = path.join(__dirname, '../src/config/version.ts');
  const versionTsContent = fs.readFileSync(versionTsPath, 'utf8');
  
  // 使用正则表达式提取版本号
  const versionMatch = versionTsContent.match(/export const APP_VERSION = '(.+)';/);
  if (!versionMatch) {
    throw new Error('无法从 version.ts 中提取版本号');
  }
  
  const version = versionMatch[1];
  log('green', `✅ 检测到版本: ${version}`);

  // 2. 更新 package.json
  log('yellow', '📦 更新 package.json...');
  const packageJsonPath = path.join(__dirname, '../package.json');
  const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf8'));
  packageJson.version = version;
  fs.writeFileSync(packageJsonPath, JSON.stringify(packageJson, null, 2) + '\n');

  // 3. 更新 tauri.conf.json
  log('yellow', '🔧 更新 tauri.conf.json...');
  const tauriConfigPath = path.join(__dirname, '../src-tauri/tauri.conf.json');
  const tauriConfig = JSON.parse(fs.readFileSync(tauriConfigPath, 'utf8'));
  tauriConfig.version = version;
  fs.writeFileSync(tauriConfigPath, JSON.stringify(tauriConfig, null, 2) + '\n');

  // 4. 更新 Cargo.toml
  log('yellow', '🦀 更新 Cargo.toml...');
  const cargoTomlPath = path.join(__dirname, '../src-tauri/Cargo.toml');
  let cargoContent = fs.readFileSync(cargoTomlPath, 'utf8');
  cargoContent = cargoContent.replace(/^version = ".*"$/m, `version = "${version}"`);
  fs.writeFileSync(cargoTomlPath, cargoContent);

  log('green', `✅ 版本同步完成！所有文件已更新为版本 ${version}`);
  log('blue', '📋 已更新的文件:');
  log('blue', '  - package.json');
  log('blue', '  - src-tauri/tauri.conf.json');
  log('blue', '  - src-tauri/Cargo.toml');

} catch (error) {
  log('red', `❌ 版本同步失败: ${error.message}`);
  process.exit(1);
}