# Tauri 应用图标生成指南

## 图标要求

### 必需文件（放在 `src-tauri/icons/` 目录）

1. **icon.png** - 主图标 (512x512 或 1024x1024 像素)
2. **icon.ico** - Windows 图标 (多尺寸 ICO 文件)
3. **icon.icns** - macOS 图标 (多尺寸 ICNS 文件)
4. **32x32.png** - 32x32 像素
5. **128x128.png** - 128x128 像素
6. **128x128@2x.png** - 256x256 像素 (高DPI)

## 在线图标生成工具

### 推荐工具：
1. **Tauri Icon Generator**: https://tauri.app/v1/guides/features/icons/
2. **ICO Convert**: https://icoconvert.com/
3. **CloudConvert**: https://cloudconvert.com/png-to-ico
4. **Favicon Generator**: https://www.favicon-generator.org/

## 使用步骤

### 方法1：使用 Tauri CLI（推荐）
```bash
# 安装 Tauri CLI
npm install -g @tauri-apps/cli

# 生成图标（从一个 1024x1024 的 PNG 文件）
tauri icon path/to/your/icon.png
```

### 方法2：手动生成
1. 准备一个 1024x1024 的 PNG 图标文件
2. 使用在线工具生成各种格式：
   - 生成 ICO 文件（包含 16x16, 32x32, 48x48, 256x256）
   - 生成 ICNS 文件（macOS）
   - 调整尺寸生成各种 PNG 文件

### 方法3：使用 ImageMagick（命令行）
```bash
# 安装 ImageMagick
# Windows: choco install imagemagick
# macOS: brew install imagemagick
# Ubuntu: sudo apt install imagemagick

# 从源图标生成各种尺寸
convert icon-source.png -resize 32x32 32x32.png
convert icon-source.png -resize 128x128 128x128.png
convert icon-source.png -resize 256x256 128x128@2x.png

# 生成 ICO 文件
convert icon-source.png -define icon:auto-resize=256,128,64,48,32,16 icon.ico
```

## 图标设计建议

1. **使用矢量图形**：确保在各种尺寸下都清晰
2. **简洁设计**：避免过于复杂的细节
3. **透明背景**：PNG 文件使用透明背景
4. **高对比度**：确保在浅色和深色背景下都可见
5. **测试各种尺寸**：确保小尺寸下仍然可识别

## 替换图标后的操作

1. 将新图标文件放入 `src-tauri/icons/` 目录
2. 确保文件名与 `tauri.conf.json` 中的配置匹配
3. 重新构建应用：`pnpm tauri build`
4. 开发模式下重启：`pnpm tauri dev` 