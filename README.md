# Clipboard Manager

<div align="center">

一个功能丰富的跨平台剪贴板管理工具，支持文本、图片和文件历史记录管理，局域网同步等。

![Tauri](https://img.shields.io/badge/Tauri_2-24C8D8?style=for-the-badge&logo=tauri&logoColor=white)
![Vue.js](https://img.shields.io/badge/Vue_3-4FC08D?style=for-the-badge&logo=vue.js&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-007ACC?style=for-the-badge&logo=typescript&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)
![SQLite](https://img.shields.io/badge/SQLite-003B57?style=for-the-badge&logo=sqlite&logoColor=white)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

</div>

## 功能特性

### 剪贴板管理
- **实时监控** — 自动捕获文本、图片和文件内容，事件驱动，无轮询
- **智能去重** — 基于 SHA-256 哈希自动过滤重复内容
- **一键粘贴** — 点击历史记录即可复制到剪贴板并自动粘贴到目标应用
- **智能粘贴** — 快捷键触发时自动回到原应用窗口执行粘贴
- **源应用追踪** — 自动识别并显示内容的来源应用程序和图标

### 历史记录
- **分类浏览** — 按文本、图片、文件分类查看
- **全文搜索** — 快速检索历史记录
- **收藏功能** — 收藏重要内容，收藏项不会被自动清理
- **分组管理** — 创建自定义分组，按类别整理剪贴板内容
- **备注管理** — 为任意条目添加备注
- **自动清理** — 支持按时间和数量自动清理，孤立图片文件自动删除
- **JSON 格式化** — 自动检测并格式化展示 JSON 内容

### 图片支持
- **图片捕获** — 自动保存剪贴板中的图片到本地
- **缩略图预览** — 自动生成缩略图，优化浏览体验
- **元数据提取** — 获取图片宽度、高度、大小、格式等信息

### 文件支持
- **文件列表监听** — 监听复制的文件列表
- **文件图标** — 根据扩展名显示对应的文件类型图标
- **文件预览** — 支持文本文件内容预览（最大 10MB）
- **打开位置** — 一键打开文件所在目录

### 局域网同步
- **多设备同步** — 通过 TCP 协议在局域网内实时同步剪贴板内容
- **主机/客户端模式** — 灵活的网络拓扑，支持密码认证
- **成员管理** — 查看在线成员，自定义成员名称
- **去重缓存** — 内置去重机制，避免重复同步

### 数据管理
- **数据导出** — 导出为 ZIP 文件，包含数据库和所有图片
- **数据导入** — 支持替换导入和合并导入（增量导入，自动去重）
- **本地存储** — 所有数据本地 SQLite 数据库存储，保护隐私

### 系统集成
- **全局快捷键** — 自定义快捷键唤醒窗口（默认 `Ctrl+Shift+V` / `Cmd+Shift+V`）
- **系统托盘** — 最小化到系统托盘，常驻后台
- **开机自启** — 支持 Windows / macOS / Linux 三平台开机自启
- **无边框窗口** — 现代化无边框 UI，始终置顶，跳过任务栏
- **macOS 全屏支持** — 通过 NSPanel 在全屏应用上显示窗口

## 技术栈

| 层级 | 技术 |
|------|------|
| 前端框架 | Vue 3 + TypeScript |
| UI 样式 | Tailwind CSS + DaisyUI |
| 组件库 | Headless UI + Heroicons |
| 桌面框架 | Tauri 2 |
| 后端语言 | Rust |
| 数据库 | SQLite (via SQLx) |
| 日志系统 | tracing + tracing-subscriber |
| 剪贴板 | tauri-plugin-clipboard + arboard |
| 键盘模拟 | rdev (Windows/Linux) + Cocoa (macOS) |
| 图片处理 | image crate |
| 数据导入导出 | zip + tempfile |

## 平台支持

| 功能 | Windows | macOS | Linux |
|------|---------|-------|-------|
| 剪贴板监听 | ✅ | ✅ | ✅ |
| 自动粘贴 | ✅ | ✅ | ✅ |
| 智能粘贴 | ✅ | ✅ | ✅ |
| 应用图标 | ✅ | ✅ | ⬜ |
| 开机自启 | ✅ | ✅ | ✅ |
| 全屏弹窗 | — | ✅ | — |
| 文件剪贴板 | ✅ | ⬜ | ⬜ |
| 打开文件位置 | ✅ | ✅ | ✅ |

## 安装使用

### 系统要求
- Windows 10/11
- macOS 11.0+
- Linux (Ubuntu 22.04+)

### 从源码构建

1. **克隆仓库**
   ```bash
   git clone https://github.com/your-username/clipboard-manager.git
   cd clipboard-manager
   ```

2. **安装依赖**
   ```bash
   # 安装前端依赖
   npm install

   # 安装 Rust（如果未安装）
   # Windows: https://rustup.rs
   # macOS/Linux:
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

3. **开发模式**
   ```bash
   npm run tauri:dev
   ```

4. **构建发布版本**
   ```bash
   npm run tauri:build
   ```

## 快速开始

1. 启动应用后自动最小化到系统托盘
2. 使用快捷键 `Ctrl+Shift+V`（macOS: `Cmd+Shift+V`）唤醒主窗口
3. 正常复制文本、图片或文件，应用自动记录
4. 在历史记录中点击任意项目即可复制并粘贴到当前应用

## 开发命令

```bash
# 开发
npm run tauri:dev          # 启动开发模式
npm run tauri:dev:fast     # 快速开发模式（需 sccache）

# 构建
npm run build              # 构建前端
npm run tauri:build        # 构建完整应用

# 版本管理
npm run sync-version       # 同步版本号
npm run release            # 交互式发布
npm run release:patch      # 版本升级 1.1.17 → 1.1.18
npm run release:minor      # 版本升级 1.1.17 → 1.2.0
npm run release:major      # 版本升级 1.1.17 → 2.0.0
```

## 项目结构

```
clipboard-manager/
├── src/                          # 前端源码
│   ├── App.vue                   # 主应用组件
│   ├── main.ts                   # 入口文件
│   ├── components/
│   │   ├── Settings.vue          # 设置面板
│   │   ├── LanQueueManager.vue   # 局域网同步管理
│   │   ├── Toast.vue             # 消息通知
│   │   └── ConfirmDialog.vue     # 确认对话框
│   ├── composables/
│   │   ├── useLogger.ts          # 前端日志系统
│   │   ├── useToast.ts           # Toast 消息
│   │   └── useImageCache.ts      # 图片缓存
│   ├── views/
│   │   └── Log.vue               # 日志查看
│   └── config/
│       └── version.ts            # 版本号（唯一定义源）
├── src-tauri/                    # Rust 后端源码
│   └── src/
│       ├── lib.rs                # 应用初始化、插件注册
│       ├── commands.rs           # Tauri 命令定义
│       ├── types.rs              # 数据类型
│       ├── window_info.rs        # 窗口信息与应用图标
│       ├── lan_queue.rs          # 局域网同步（TCP）
│       ├── macos_paste.rs        # macOS 智能粘贴
│       ├── icon_cache.rs         # 图标缓存
│       ├── resource_manager.rs   # 资源管理
│       └── logging.rs            # 日志系统
├── scripts/
│   ├── release.cjs               # 发布脚本
│   └── sync-version.cjs          # 版本同步脚本
└── .github/workflows/
    └── release.yml               # CI/CD 自动构建发布
```

## 数据存储

| 数据 | 路径 |
|------|------|
| 数据库 | `{应用数据目录}/clipboard.db` |
| 图片 | `{应用数据目录}/images/` |
| 日志 | `{应用数据目录}/logs/` |
| 设置 | `{应用数据目录}/clipboard_settings.json` |

## 贡献指南

欢迎贡献代码！请遵循以下步骤：

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 创建 Pull Request

## 许可证

[MIT License](LICENSE) Copyright (c) 2024 Clipboard Manager

---

<div align="center">

如果这个项目对你有帮助，请给个 ⭐

Made by [HughArch]

</div>
