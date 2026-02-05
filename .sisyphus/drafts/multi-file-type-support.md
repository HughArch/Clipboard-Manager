# Draft: 多类型文件历史记录支持

## 需求确认 (FINAL)

| 需求项 | 决策 |
|--------|------|
| 支持范围 | 全部文件类型 |
| 存储策略 | 路径引用模式（不复制文件） |
| 文件失效处理 | 灰显并标记 |
| UI展示 | 文件类型图标、文件名、文件大小 |
| 点击行为 | 双击粘贴文件，Ctrl+双击粘贴路径 |
| UI组织 | 混合在现有标签页（按时间排序） |
| 平台 | 仅Windows |

## 技术架构分析

### 剪贴板插件支持

**发现**: 项目使用的 `tauri-plugin-clipboard` v2.1 (来自CrossCopy) **原生支持文件监听**！

```typescript
import { onFilesUpdate } from 'tauri-plugin-clipboard-api'

// 监听文件剪贴板变化
onFilesUpdate((files: string[]) => {
  // files 是文件路径数组
  console.log('Files copied:', files)
})
```

这大大简化了实现 - 无需自己实现CF_HDROP读取！

### 数据库结构

**当前表**: `clipboard_history`
```sql
CREATE TABLE clipboard_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    content TEXT NOT NULL,           -- 文本存内容，图片/文件存路径
    type TEXT NOT NULL,               -- "text" | "image" | "file" (新增)
    timestamp TEXT NOT NULL,
    is_favorite INTEGER NOT NULL DEFAULT 0,
    image_path TEXT,                  -- 图片路径
    source_app_name TEXT,
    source_app_icon TEXT,
    thumbnail_data TEXT,              -- 缩略图/文件图标
    metadata TEXT                     -- JSON: 扩展信息
)
```

**文件类型的字段使用**:
- `content`: 存储文件路径（单文件）或JSON数组（多文件）
- `type`: "file"
- `image_path`: 保持NULL（文件不需要）
- `thumbnail_data`: 存储文件图标的base64
- `metadata`: JSON格式存储文件元信息
  ```json
  {
    "files": [
      {
        "path": "C:\\path\\to\\file.pdf",
        "name": "file.pdf",
        "extension": "pdf",
        "size": 1234567,
        "exists": true
      }
    ],
    "file_count": 1
  }
  ```

### 代码位置参考

| 功能 | 文件 | 行号 |
|------|------|------|
| 数据库初始化 | `src-tauri/src/lib.rs` | 56-67 |
| 保存图片 | `src-tauri/src/commands.rs` | 1390-1453 |
| 复制图片 | `src-tauri/src/commands.rs` | 1503-1568 |
| 清理逻辑 | `src-tauri/src/commands.rs` | 150-265 |
| 剪贴板监听 | `src/App.vue` | 2198-2400 |
| 文本监听器 | `src/App.vue` | 2201-2327 |
| 图片监听器 | `src/App.vue` | 2329-2400+ |

### 现有依赖可复用

- `clipboard-win`: CF_HDROP文件写入（已用于图片）
- Windows文件图标：需新增 Shell API 调用

## 实现计划

### Phase 1: 后端文件处理 (Rust)

#### 1.1 新增文件复制命令
```rust
// src-tauri/src/commands.rs
#[tauri::command]
pub async fn copy_files_to_clipboard(file_paths: Vec<String>) -> Result<(), String>
```

使用已有的 `clipboard-win` CF_HDROP 逻辑。

#### 1.2 新增文件元信息获取
```rust
#[tauri::command]
pub async fn get_file_metadata(file_path: String) -> Result<FileMetadata, String>
// 返回: 文件名、大小、扩展名、是否存在
```

#### 1.3 新增文件图标获取 (Windows)
```rust
#[tauri::command]
pub async fn get_file_icon(file_path: String) -> Result<String, String>
// 使用 Shell API 获取系统图标，返回 base64
```

### Phase 2: 前端文件监听 (Vue)

#### 2.1 添加文件监听器
```typescript
// 在 onMounted 中添加
import { onFilesUpdate } from 'tauri-plugin-clipboard-api'

unlistenClipboardFiles = await onFilesUpdate(async (files: string[]) => {
  // 处理文件剪贴板事件
})
```

#### 2.2 文件保存逻辑
- 检查重复（通过文件路径）
- 获取文件元信息
- 获取文件图标
- 插入数据库
- 更新内存列表

### Phase 3: 前端UI渲染

#### 3.1 文件条目渲染
- 显示文件图标（从thumbnail_data）
- 显示文件名
- 显示文件大小
- 显示文件状态（存在/失效）

#### 3.2 文件操作
- 双击: 粘贴文件到当前应用
- Ctrl+双击: 粘贴文件路径
- 右键菜单: 添加"打开文件位置"选项

### Phase 4: 筛选与搜索

#### 4.1 标签页过滤
- "全部"标签页: 包含文件
- 新增"文件"过滤能力（或在现有标签页中过滤）

#### 4.2 搜索
- 支持按文件名搜索

## 工作项清单

### Rust后端
- [ ] 1. 添加 `copy_files_to_clipboard` 命令
- [ ] 2. 添加 `get_file_metadata` 命令  
- [ ] 3. 添加 `get_file_icon` 命令 (Windows Shell API)
- [ ] 4. 添加 `check_file_exists` 命令
- [ ] 5. 在 lib.rs 中注册新命令

### Vue前端
- [ ] 6. 导入 `onFilesUpdate` 并添加监听器
- [ ] 7. 实现文件保存逻辑（去重、保存、更新UI）
- [ ] 8. 实现文件条目渲染组件
- [ ] 9. 实现文件失效状态检测与灰显
- [ ] 10. 修改 `copyToClipboard` 支持文件类型
- [ ] 11. 修改搜索逻辑支持文件名搜索

### 测试
- [ ] 12. 测试单文件复制/粘贴
- [ ] 13. 测试多文件复制/粘贴
- [ ] 14. 测试文件失效处理
- [ ] 15. 测试与图片/文本混合显示

## 风险与注意事项

1. **文件失效检测时机**: 
   - 方案A: 仅在选中/显示时检测
   - 方案B: 后台定期检测（影响性能）
   - 建议: 方案A + 使用时检测

2. **多文件处理**:
   - 将多文件作为单条记录存储
   - 显示时展示文件数量和第一个文件名

3. **大文件夹**:
   - 如果用户复制包含大量文件的文件夹
   - 建议: 限制显示/存储的文件路径数量

4. **权限问题**:
   - 某些文件可能无法获取图标
   - 使用默认图标作为fallback
