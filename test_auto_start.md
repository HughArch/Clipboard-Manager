# 自启动功能测试指南

## 功能说明
已成功实现跟随系统启动的功能，参考快捷键的实现方式，使用自定义命令处理系统启动设置。

## 实现的功能

### 1. 后端命令
- `set_auto_start(enable: bool)` - 设置自启动状态
- `get_auto_start_status()` - 获取当前自启动状态

### 2. 系统集成
- **Windows**: 通过注册表 `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run` 管理自启动
- **非Windows系统**: 提供占位实现，返回不支持提示

### 3. 前端集成
- 设置界面中的"Start with system"复选框
- 保存设置时自动应用自启动配置
- 加载设置时同步显示实际的自启动状态

## 测试步骤

### 1. 基本功能测试
1. 运行应用程序
2. 打开设置界面
3. 勾选"Start with system"选项
4. 点击保存
5. 检查Windows注册表中是否添加了启动项

### 2. 注册表验证
打开命令提示符，运行：
```cmd
reg query "HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Run" /v ClipboardManager
```

### 3. 状态同步测试
1. 手动修改注册表中的启动项
2. 重新打开应用设置
3. 验证界面显示是否与实际状态一致

### 4. 取消自启动测试
1. 取消勾选"Start with system"
2. 保存设置
3. 验证注册表中的启动项是否被移除

## 技术实现细节

### 1. 权限配置
在 `src-tauri/capabilities/main.json` 中添加了相应的命令权限：
- `allow-set-auto-start`
- `allow-get-auto-start-status`

### 2. 命令注册
在 `src-tauri/build.rs` 中注册了新的命令。

### 3. 应用启动时的处理
在应用启动时会：
- 加载保存的设置
- 根据配置文件中的 `auto_start` 字段应用自启动设置
- 如果没有配置文件，默认不启用自启动

### 4. 错误处理
- 对于非Windows系统，提供友好的错误提示
- 处理注册表操作可能的失败情况
- 在删除不存在的注册表项时不报错

## 注意事项
1. 自启动功能目前仅支持Windows系统
2. 需要管理员权限来修改注册表（通常用户权限即可访问HKCU）
3. 应用程序路径变化时需要重新设置自启动 