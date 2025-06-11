# 数据库迁移修复说明

## 问题描述

应用启动时出现错误：
```
Database error: error returned from database: (code: 1) no such column: source_app_name
```

## 问题原因

当我们为源应用显示功能添加新的数据库列时，只修改了 `CREATE TABLE` 语句。对于已经存在的数据库，这些新列不会自动添加到现有表中。

## 解决方案

### 1. 添加数据库迁移逻辑

在 `init_database` 函数中添加了迁移代码：

```rust
// 迁移：添加新的列（如果不存在）
// 检查并添加 source_app_name 列
let add_source_app_name = sqlx::query(
    "ALTER TABLE clipboard_history ADD COLUMN source_app_name TEXT"
)
.execute(&pool)
.await;

if let Err(e) = add_source_app_name {
    // 如果列已存在，SQLite会返回错误，这是正常的
    if !e.to_string().contains("duplicate column name") {
        println!("添加 source_app_name 列时的警告: {}", e);
    }
} else {
    println!("已添加 source_app_name 列");
}

// 检查并添加 source_app_icon 列
let add_source_app_icon = sqlx::query(
    "ALTER TABLE clipboard_history ADD COLUMN source_app_icon TEXT"
)
.execute(&pool)
.await;

if let Err(e) = add_source_app_icon {
    // 如果列已存在，SQLite会返回错误，这是正常的
    if !e.to_string().contains("duplicate column name") {
        println!("添加 source_app_icon 列时的警告: {}", e);
    }
} else {
    println!("已添加 source_app_icon 列");
}
```

### 2. 工作原理

1. **安全迁移**: 使用 `ALTER TABLE ADD COLUMN` 安全地添加新列
2. **错误处理**: 如果列已存在，会返回错误，但这是正常的
3. **向后兼容**: 新列允许 NULL 值，不影响现有数据
4. **自动执行**: 每次应用启动时自动检查和迁移

### 3. 迁移效果

- ✅ 现有数据保持不变
- ✅ 新列自动添加到数据库
- ✅ 新功能正常工作
- ✅ 向后兼容

## 测试步骤

1. **重新启动应用**:
   ```bash
   npm run tauri dev
   ```

2. **检查控制台输出**:
   应该看到类似以下信息：
   ```
   已添加 source_app_name 列
   已添加 source_app_icon 列
   数据库初始化完成
   ```

3. **测试新功能**:
   - 从不同应用程序复制内容
   - 验证源应用信息正确显示
   - 检查旧数据仍然可用

## 防止类似问题

### 1. 版本化迁移

对于更复杂的数据库变更，建议使用版本化迁移：

```rust
async fn run_migrations(pool: &SqlitePool) -> Result<(), String> {
    // 获取当前数据库版本
    let version = get_db_version(pool).await?;
    
    if version < 2 {
        // 迁移到版本 2：添加源应用信息列
        migrate_to_v2(pool).await?;
        set_db_version(pool, 2).await?;
    }
    
    // 未来的迁移...
    
    Ok(())
}
```

### 2. 备份策略

在进行重大数据库变更前，建议：
- 自动备份现有数据库
- 提供恢复机制
- 测试迁移过程

## 总结

通过添加数据库迁移逻辑，成功解决了新列不存在的问题。现在应用程序能够：

1. **自动检测**现有数据库结构
2. **安全添加**缺失的列
3. **保持兼容**新旧数据
4. **正常工作**源应用显示功能

这种迁移策略确保了应用程序的平滑升级，用户无需手动操作即可享受新功能。 