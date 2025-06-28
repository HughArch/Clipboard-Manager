# macOS 安装说明

## 📱 系统要求

- **macOS 10.13** 或更高版本
- **支持架构**: Intel (x86_64) 和 Apple Silicon (ARM64)

## 📦 安装步骤

### 1. 下载应用
从 [Releases](https://github.com/your-repo/releases) 页面下载最新的 `.dmg` 文件。

### 2. 安装应用
1. 双击下载的 `.dmg` 文件
2. 将 `Clipboard Manager` 拖拽到 `Applications` 文件夹
3. 弹出 DMG 镜像

### 3. 解决"应用已损坏"问题

如果在首次启动时遇到**"应用已损坏"**的提示，这是因为应用未经过苹果公证。请按以下步骤解决：

#### 方法一：命令行解决（推荐）
```bash
# 移除应用的隔离属性
sudo xattr -rd com.apple.quarantine /Applications/Clipboard\ Manager.app

# 或者使用完整路径
sudo xattr -rd com.apple.quarantine "/Applications/Clipboard Manager.app"
```

#### 方法二：系统设置解决
1. 在 Finder 中找到应用
2. 右键点击应用，选择"打开"
3. 在弹出的对话框中点击"打开"
4. 输入管理员密码确认

#### 方法三：隐私与安全设置
1. 打开 `系统偏好设置` > `安全性与隐私`
2. 在 `通用` 标签页中点击 `仍要打开`
3. 输入管理员密码确认

## 🔧 常见问题

### Q: 为什么会提示"应用已损坏"？
**A**: 这是 macOS 的安全机制。由于这是开源应用，没有经过苹果的付费公证流程，系统会显示此警告。应用本身是安全的。

### Q: Universal Binary 是什么意思？
**A**: Universal Binary 意味着一个应用文件同时包含 Intel 和 Apple Silicon 的代码，在两种架构的 Mac 上都能原生运行，无需 Rosetta 转译。

### Q: 如何验证应用的完整性？
**A**: 你可以在终端中使用以下命令验证应用：
```bash
# 检查应用签名
codesign -dv /Applications/Clipboard\ Manager.app

# 检查应用架构
file /Applications/Clipboard\ Manager.app/Contents/MacOS/Clipboard\ Manager
```

### Q: 如何完全卸载应用？
**A**: 
1. 删除应用：`/Applications/Clipboard Manager.app`
2. 删除配置文件：`~/Library/Application Support/com.clipboard-manager.app/`
3. 删除偏好设置：`~/Library/Preferences/com.clipboard-manager.app.plist`

## 🛡️ 安全说明

- 此应用是开源的，你可以在 GitHub 上查看所有源代码
- 应用只访问剪贴板内容，不会收集或上传任何个人数据
- 所有数据都存储在本地，不会发送到外部服务器

## 🐛 问题反馈

如果遇到任何问题，请在 GitHub Issues 中反馈，包含以下信息：

1. macOS 版本
2. Mac 型号（Intel 或 Apple Silicon）
3. 错误截图或日志
4. 重现步骤

---

💡 **提示**: 如果问题仍然存在，可以尝试重新下载最新版本，或者使用 Homebrew 等包管理器安装。 