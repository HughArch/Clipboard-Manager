# macOS 权限配置指南

## 问题描述
在 macOS 上使用自动粘贴功能时，需要授予应用程序"辅助功能"权限，否则会导致程序崩溃或无法正常工作。

## 解决方案

### 1. 授予辅助功能权限

1. 打开 **系统偏好设置** (System Preferences)
2. 点击 **安全性与隐私** (Security & Privacy)
3. 选择 **隐私** (Privacy) 选项卡
4. 在左侧列表中找到 **辅助功能** (Accessibility)
5. 点击左下角的 🔒 锁图标并输入管理员密码
6. 点击 **+** 按钮添加应用程序
7. 找到并选择 `Clipboard Manager.app`
8. 确保应用程序旁边的复选框已勾选

### 2. 测试权限是否生效

1. 重启 Clipboard Manager 应用程序
2. 尝试使用自动粘贴功能
3. 如果仍然有问题，尝试以下步骤：
   - 将应用从辅助功能列表中移除
   - 重新添加应用
   - 重启应用

### 3. 开发模式权限配置

如果你在开发模式下运行应用（使用 `cargo tauri dev`），你需要为以下两个程序都授予权限：

1. **Terminal** 或你使用的终端应用
2. **Clipboard Manager** 应用程序本身

### 4. 备用解决方案

如果权限配置后仍然有问题，你可以：

1. **使用系统通知提醒**：应用程序可以显示通知，提醒用户手动按 Cmd+V
2. **只复制到剪贴板**：应用程序只将内容复制到剪贴板，用户手动粘贴
3. **使用 AppleScript**：通过 AppleScript 实现更稳定的自动粘贴

## 常见问题

### Q: 为什么需要辅助功能权限？
A: macOS 出于安全考虑，不允许应用程序随意模拟键盘和鼠标操作。辅助功能权限专门用于这类需求。

### Q: 授予权限是否安全？
A: 是的，Clipboard Manager 是一个本地应用，只用于模拟粘贴操作，不会进行其他恶意活动。

### Q: 如果不想授予权限怎么办？
A: 你可以使用应用程序只复制到剪贴板的功能，然后手动按 Cmd+V 进行粘贴。

## 权限验证代码

你可以在代码中添加权限检查：

```rust
#[cfg(target_os = "macos")]
fn check_accessibility_permission() -> bool {
    // 这里可以添加检查代码
    true
}
```

## 注意事项

- 每次应用程序更新后，可能需要重新授予权限
- 如果应用程序签名发生变化，也需要重新配置权限
- 建议在应用程序首次启动时显示权限配置指南 