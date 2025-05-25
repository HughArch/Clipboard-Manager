# 剪贴板历史数据自动清理功能

## 功能概述

已成功实现根据设置中的最大历史条目和最大历史时间自动删除过期数据的功能，有效防止历史数据过多占用存储空间。

## 实现的功能

### 1. 自动清理机制

#### 按时间清理
- 删除超过指定天数的历史记录
- **保护收藏项**：收藏的项目不会被时间清理删除
- 使用 `max_history_time` 设置（单位：天）

#### 按数量清理
- 保留最新的指定数量记录
- **收藏项不计入数量限制**：收藏的项目不占用普通历史记录的数量配额
- 删除最旧的非收藏记录
- 使用 `max_history_items` 设置

### 2. 触发时机

#### 应用启动时
- 自动执行一次数据清理
- 使用当前设置或默认设置进行清理

#### 保存设置时
- 每次保存设置后自动执行清理
- 立即应用新的清理规则

#### 手动触发
- 提供 `cleanup_history` 命令供前端调用
- 可以随时手动执行清理操作

### 3. 清理逻辑

```sql
-- 1. 按时间清理（保留收藏项）
DELETE FROM clipboard_history 
WHERE timestamp < ? AND is_favorite = 0

-- 2. 查询当前非收藏记录数量
SELECT COUNT(*) as count FROM clipboard_history WHERE is_favorite = 0

-- 3. 按数量清理（如果超出限制）
DELETE FROM clipboard_history 
WHERE is_favorite = 0 
AND id IN (
    SELECT id FROM clipboard_history 
    WHERE is_favorite = 0 
    ORDER BY timestamp ASC 
    LIMIT ?
)
```

## 技术实现

### 1. 后端实现 (`src-tauri/src/lib.rs`)

#### 核心函数
```rust
async fn cleanup_expired_data(app: &AppHandle, settings: &AppSettings) -> Result<(), String>
```

#### 新增命令
```rust
#[tauri::command]
async fn cleanup_history(app: AppHandle) -> Result<(), String>
```

#### 依赖添加
- `chrono` - 用于日期时间计算
- `sqlx` - 数据库操作（已有，添加了 Row trait）

### 2. 权限配置

#### `src-tauri/capabilities/main.json`
```json
{
  "identifier": "allow-cleanup-history",
  "description": "Allows cleaning up expired clipboard history",
  "commands": ["cleanup_history"]
}
```

#### `src-tauri/build.rs`
```rust
.commands(&["...", "cleanup_history"])
```

### 3. 集成点

#### 应用启动时清理
```rust
// 在 setup 函数中
let _ = cleanup_expired_data(&app_handle, &settings).await;
```

#### 保存设置时清理
```rust
// 在 save_settings 函数中
cleanup_expired_data(&app, &settings).await?;
```

## 使用示例

### 1. 设置配置
```json
{
  "max_history_items": 100,    // 最多保留100条非收藏记录
  "max_history_time": 30,      // 保留30天内的记录
  "hotkey": "Ctrl+Shift+V",
  "auto_start": false
}
```

### 2. 前端手动触发清理
```typescript
import { invoke } from '@tauri-apps/api/core'

// 手动执行清理
try {
  await invoke('cleanup_history')
  console.log('清理完成')
} catch (error) {
  console.error('清理失败:', error)
}
```

## 清理策略

### 1. 收藏项保护
- ✅ 收藏的项目永远不会被自动删除
- ✅ 收藏项不占用普通历史记录的数量配额
- ✅ 即使超过时间限制，收藏项也会保留

### 2. 清理优先级
1. **时间优先**：首先删除超过时间限制的非收藏记录
2. **数量控制**：然后按数量限制删除最旧的非收藏记录
3. **保护机制**：收藏项在任何情况下都不会被删除

### 3. 默认设置
- 最大历史条目：100条
- 最大历史时间：30天
- 在没有用户设置时使用默认值进行清理

## 性能考虑

### 1. 数据库索引
- 已创建 `idx_clipboard_content` 索引
- 按 `timestamp` 排序查询效率高

### 2. 批量操作
- 使用 SQL 批量删除，避免逐条操作
- 先查询再删除，确保操作精确

### 3. 异步执行
- 所有清理操作都是异步的
- 不会阻塞主线程或用户界面

## 错误处理

### 1. 数据库错误
- 捕获并返回详细的错误信息
- 区分时间清理和数量清理的错误

### 2. 设置加载失败
- 使用默认设置继续执行清理
- 确保清理功能始终可用

### 3. 日志记录
- 错误信息包含具体的失败原因
- 便于调试和问题排查

## 测试建议

### 1. 功能测试
1. 设置较小的历史条目数量（如5条）
2. 添加多条测试数据
3. 收藏部分数据
4. 保存设置，观察清理效果

### 2. 时间测试
1. 设置较短的历史时间（如1天）
2. 修改数据库中的时间戳为过期时间
3. 触发清理，验证过期数据被删除

### 3. 收藏保护测试
1. 收藏一些历史记录
2. 设置很小的数量限制
3. 验证收藏项不被删除

## 注意事项

1. **数据安全**：清理操作不可逆，请确保设置合理的参数
2. **收藏保护**：重要数据请及时收藏以防被清理
3. **性能影响**：大量数据清理可能需要一些时间
4. **设置调整**：修改清理参数后会立即生效

## 未来扩展

1. **清理统计**：返回清理的记录数量
2. **清理日志**：记录清理操作的详细信息
3. **用户确认**：大量删除前的用户确认机制
4. **增量清理**：定时小批量清理而非一次性大量清理 