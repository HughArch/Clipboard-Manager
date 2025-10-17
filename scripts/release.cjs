#!/usr/bin/env node

const { execSync } = require('child_process');
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

// 获取命令行参数
const args = process.argv.slice(2);
const versionType = args[0]; // major, minor, patch 或具体版本号

if (!versionType) {
  log('red', '❌ 请指定版本类型: major, minor, patch 或具体版本号');
  log('yellow', '示例: npm run release patch');
  log('yellow', '示例: npm run release 1.2.0');
  process.exit(1);
}

try {
  log('blue', '🚀 开始发布流程...');

  // 1. 检查工作目录是否干净
  // log('yellow', '📋 检查工作目录状态...');
  // try {
  //   execSync('git diff --exit-code', { stdio: 'pipe' });
  //   execSync('git diff --cached --exit-code', { stdio: 'pipe' });
  // } catch (error) {
  //   log('red', '❌ 工作目录不干净，请先提交所有更改');
  //   process.exit(1);
  // }

  // 2. 拉取最新代码
  log('yellow', '📥 拉取最新代码...');
  execSync('git pull origin main', { stdio: 'inherit' });

  // 3. 更新版本号
  log('yellow', '📝 更新版本号...');
  
  let newVersion;
  if (['major', 'minor', 'patch'].includes(versionType)) {
    // 使用 npm version 命令
    const result = execSync(`npm version ${versionType} --no-git-tag-version`, { encoding: 'utf8' });
    newVersion = result.trim().substring(1); // 移除 'v' 前缀
  } else {
    // 直接设置版本号
    newVersion = versionType;
    execSync(`npm version ${newVersion} --no-git-tag-version`, { stdio: 'inherit' });
  }

  // 4. 更新前端版本配置文件（作为版本的唯一来源）
  log('yellow', '🎨 更新前端版本配置...');
  const versionTsPath = path.join(__dirname, '../src/config/version.ts');
  let versionTsContent = fs.readFileSync(versionTsPath, 'utf8');
  versionTsContent = versionTsContent.replace(/export const APP_VERSION = '.*';/, `export const APP_VERSION = '${newVersion}';`);
  fs.writeFileSync(versionTsPath, versionTsContent);

  // 5. 同步版本到其他配置文件
  log('yellow', '🔄 同步版本到其他配置文件...');
  execSync('npm run sync-version', { stdio: 'inherit' });

  // 6. 提交更改
  log('yellow', '💾 提交版本更改...');
  execSync(`git add .`);
  execSync(`git commit -m "chore: release v${newVersion}"`);

  // 7. 创建标签
  log('yellow', '🏷️ 创建版本标签...');
  execSync(`git tag v${newVersion}`);

  // 8. 推送到远程仓库
  log('yellow', '🚀 推送到远程仓库...');
  execSync('git push');
  execSync('git push --tags');

  log('green', `✅ 版本 v${newVersion} 发布成功！`);
  log('blue', '🔗 GitHub Actions 将自动构建并创建 Release');
  log('blue', '📋 请访问 GitHub 仓库查看构建进度');

} catch (error) {
  log('red', `❌ 发布失败: ${error.message}`);
  process.exit(1);
}