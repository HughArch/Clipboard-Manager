# 版本管理机制说明

## 概述

本项目采用统一的版本管理机制，以 `src/config/version.ts` 作为版本信息的唯一来源，确保所有配置文件中的版本信息保持一致。

## 版本管理架构

### 1. 版本源文件
- **文件位置**: `src/config/version.ts`
- **作用**: 定义应用版本号的唯一来源
- **内容**: 导出 `APP_VERSION` 常量和相关工具函数

### 2. 同步目标文件
- `package.json` - npm 包版本
- `src-tauri/tauri.conf.json` - Tauri 应用版本
- `src-tauri/Cargo.toml` - Rust 包版本

## 使用方法

### 手动同步版本
当你修改了 `src/config/version.ts` 中的版本号后，运行以下命令同步到其他文件：

```bash
npm run sync-version
```

### 发布新版本
使用 release 脚本自动更新版本并发布：

```bash
# 补丁版本 (1.0.0 -> 1.0.1)
npm run release:patch

# 次要版本 (1.0.0 -> 1.1.0)
npm run release:minor

# 主要版本 (1.0.0 -> 2.0.0)
npm run release:major

# 指定版本号
npm run release 1.2.3
```

## 工作流程

### 发布流程
1. `release.cjs` 脚本更新 `package.json` 版本
2. 同步更新 `src/config/version.ts` 中的 `APP_VERSION`
3. 运行 `sync-version.cjs` 脚本同步到其他配置文件
4. 提交更改并创建 Git 标签
5. 推送到远程仓库

### 同步流程
1. 从 `src/config/version.ts` 读取版本号
2. 更新 `package.json` 的 `version` 字段
3. 更新 `src-tauri/tauri.conf.json` 的 `version` 字段
4. 更新 `src-tauri/Cargo.toml` 的 `version` 字段

## 优势

1. **统一管理**: 版本信息集中在一个文件中管理
2. **自动同步**: 脚本自动确保所有文件版本一致
3. **类型安全**: TypeScript 提供类型检查
4. **易于维护**: 减少手动更新多个文件的错误风险
5. **前端可用**: 前端代码可以直接引用版本信息

## 注意事项

- 不要手动修改 `package.json`、`tauri.conf.json` 或 `Cargo.toml` 中的版本号
- 版本更改应该通过修改 `src/config/version.ts` 或使用 release 脚本完成
- 在提交代码前，确保运行 `npm run sync-version` 保持版本同步