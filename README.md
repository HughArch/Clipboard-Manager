# 📋 Clipboard Manager

<div align="center">

一个功能强大的跨平台剪贴板管理工具，支持文本和图片历史记录管理。

![Tauri](https://img.shields.io/badge/Tauri-24C8D8?style=for-the-badge&logo=tauri&logoColor=white)
![Vue.js](https://img.shields.io/badge/Vue.js-4FC08D?style=for-the-badge&logo=vue.js&logoColor=white)
![TypeScript](https://img.shields.io/badge/TypeScript-007ACC?style=for-the-badge&logo=typescript&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-000000?style=for-the-badge&logo=rust&logoColor=white)

</div>

## ✨ 功能特性

- 🔄 **实时监控剪贴板**：自动捕获文本和图片内容
- 📝 **文本历史记录**：保存所有复制的文本内容
- 🖼️ **图片历史记录**：支持图片保存和预览缩略图
- ⭐ **收藏功能**：可以收藏重要的剪贴板内容
- 🔍 **快速搜索**：快速查找历史记录
- ⌨️ **全局快捷键**：支持自定义快捷键唤醒窗口（默认 Ctrl+Shift+V）
- 🚀 **一键粘贴**：点击即可复制到剪贴板并自动粘贴
- 🗂️ **智能分类**：自动识别来源应用程序并显示图标
- 🧹 **自动清理**：支持按时间和数量自动清理历史记录
- 💾 **本地存储**：所有数据本地存储，保护隐私
- 🎨 **现代界面**：美观的用户界面，支持系统托盘
- 🔧 **开机自启**：可选择开机自动启动

## 🛠️ 技术栈

### 前端
- **Vue 3** - 渐进式 JavaScript 框架
- **TypeScript** - 类型安全的 JavaScript
- **Vite** - 快速的前端构建工具
- **Element Plus** - Vue 3 组件库

### 后端
- **Tauri** - 使用 Web 技术构建桌面应用
- **Rust** - 系统级编程语言
- **SQLite** - 轻量级数据库
- **SQLx** - Rust 异步 SQL 工具包

### 核心依赖
- **arboard** - 跨平台剪贴板访问
- **image** - 图片处理库
- **enigo** - 键盘鼠标模拟
- **winapi** - Windows API 绑定（Windows 平台）

## 📦 安装使用

### 系统要求
- Windows 10/11
- macOS 10.15+ （待支持）
- Linux (Ubuntu 18.04+)（待支持）

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

# 安装 Rust (如果未安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

3. **开发模式运行**
```bash
npm run tauri dev
```

4. **构建发布版本**
```bash
npm run tauri build
```

## 🚀 快速开始

1. 启动应用程序
2. 程序会自动最小化到系统托盘
3. 使用自定义快捷键唤醒主窗口
4. 复制任何文本或图片，程序会自动记录
5. 在历史记录中点击任意项目即可复制并粘贴

## ⚙️ 配置选项

在设置页面可以配置：

- **历史记录数量限制**：设置最大保存的记录数量
- **历史记录时间限制**：设置记录保存的天数
- **全局快捷键**：自定义唤醒窗口的快捷键
- **开机自启动**：设置是否开机自动启动

## 📁 文件存储

- **数据库文件**：`{程序目录}/clipboard.db`
- **图片文件**：`{程序目录}/images/`
- **设置文件**：`%APPDATA%/clipboard_settings.json` (Windows)

## 🤝 贡献指南

欢迎贡献代码！请遵循以下步骤：

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/AmazingFeature`)
3. 提交更改 (`git commit -m 'Add some AmazingFeature'`)
4. 推送到分支 (`git push origin feature/AmazingFeature`)
5. 开启 Pull Request


## 📞 联系方式

如果你有任何问题或建议，请通过以下方式联系：

- 🐛 Issues: [GitHub Issues](https://github.com/your-username/clipboard-manager/issues)

---

<div align="center">

**如果这个项目对你有帮助，请给它一个 ⭐**

Made with ❤️ by [HughArch]

</div>
