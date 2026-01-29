# AGENTS.md

Guidance for AI coding agents working in this repository.

## Project Overview

Cross-platform clipboard manager built with **Tauri 2.x** (Rust backend) and **Vue 3 + TypeScript** (frontend).

## Build/Dev Commands

```bash
# Install dependencies
npm install

# Development (recommended)
npm run tauri:dev

# Fast development (with sccache)
npm run tauri:dev:fast

# Build frontend only
npm run build

# Build complete application
npm run tauri:build

# Type check frontend
npx vue-tsc --noEmit

# Check Rust code
cargo check --manifest-path src-tauri/Cargo.toml

# Run Rust clippy
cargo clippy --manifest-path src-tauri/Cargo.toml
```

### Version Management

```bash
npm run sync-version        # Sync package.json and Cargo.toml versions
npm run release:patch       # 1.1.10 -> 1.1.11
npm run release:minor       # 1.1.10 -> 1.2.0
npm run release:major       # 1.1.10 -> 2.0.0
```

## Project Structure

```
src/                        # Vue 3 frontend
  App.vue                   # Main component with clipboard logic
  main.ts                   # Vue app entry point
  composables/              # Vue composables (useToast, useLogger, etc.)
  components/               # Vue components (Settings, Toast, etc.)
  views/                    # View components
  style.css                 # Global styles (Tailwind)

src-tauri/src/              # Rust backend
  lib.rs                    # Tauri app entry, plugin setup, database init
  commands.rs               # Tauri commands (frontend -> backend)
  types.rs                  # Shared type definitions
  resource_manager.rs       # Image and resource management
  icon_cache.rs             # App icon caching
  window_info.rs            # Window information (platform-specific)
  logging.rs                # Structured logging with tracing
  macos_paste.rs            # macOS-specific paste functionality
```

## Code Style Guidelines

### TypeScript/Vue (Frontend)

**tsconfig.json settings** (strict mode enabled):
- `strict: true`
- `noUnusedLocals: true`
- `noUnusedParameters: true`
- `noFallthroughCasesInSwitch: true`

**Imports** - Order: Vue core -> external libs -> local modules -> types
```typescript
import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useToast } from './composables/useToast'
import type { AppSettings } from './types'
```

**Vue Components** - Use `<script setup lang="ts">` syntax
```vue
<script setup lang="ts">
import { ref } from 'vue'

interface Props {
  title: string
  count?: number
}

const props = defineProps<Props>()
const emit = defineEmits<{
  (e: 'update', value: number): void
}>()
</script>
```

**Naming Conventions**:
- Components: PascalCase (`Settings.vue`, `ConfirmDialog.vue`)
- Composables: camelCase with `use` prefix (`useToast`, `useLogger`)
- Variables/functions: camelCase
- Constants: SCREAMING_SNAKE_CASE (`MAX_MEMORY_ITEMS`)
- Interfaces: PascalCase (`AppSettings`, `SourceAppInfo`)

**Error Handling**:
```typescript
try {
  await invoke('save_settings', { settings })
  logger.info('Settings saved successfully')
} catch (error) {
  logger.error('Failed to save settings', { error: String(error) })
  throw error
}
```

### Rust (Backend)

**Naming Conventions**:
- Functions/variables: snake_case (`init_database`, `settings_file_path`)
- Types/Structs: PascalCase (`AppSettings`, `DatabaseState`)
- Constants: SCREAMING_SNAKE_CASE (`SETTINGS_FILE`)
- Modules: snake_case (`resource_manager`, `window_info`)

**Tauri Commands** - Use `#[tauri::command]` macro:
```rust
#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    // Implementation
}
```

**Error Handling** - Return `Result<T, String>` for Tauri commands:
```rust
pub async fn load_settings(app: AppHandle) -> Result<AppSettings, String> {
    let path = settings_file_path().map_err(|e| format!("Path error: {}", e))?;
    // ...
}
```

**Platform-Specific Code** - Use conditional compilation:
```rust
#[cfg(target_os = "macos")]
mod macos_paste;

#[cfg(windows)]
use winapi::um::winuser;
```

**Logging** - Use `tracing` macros:
```rust
tracing::info!("Database initialized");
tracing::error!("Failed to connect: {}", e);
tracing::debug!("Processing item: {:?}", item);
```

## UI Framework

- **Tailwind CSS** - Utility-first styling
- **DaisyUI** - Component library on top of Tailwind
- **Headless UI** - Unstyled accessible components
- **Heroicons** - Icon library (use `@heroicons/vue`)

## Database

- **SQLite** via `@tauri-apps/plugin-sql` (frontend) and `sqlx` (backend)
- Main table: `clipboard_history`
- Groups table: `groups`

## Key Dependencies

### Frontend
- `vue` ^3.5 - UI framework
- `@tauri-apps/api` ^2.0 - Tauri API bindings
- `@tauri-apps/plugin-sql` ^2.2 - SQLite access
- `tauri-plugin-clipboard-api` ^2.1 - Clipboard access
- `@vueuse/core` ^13 - Vue composition utilities

### Backend (Cargo.toml)
- `tauri` 2.x - Desktop framework
- `sqlx` 0.8 - Async SQL toolkit
- `tokio` 1.x - Async runtime
- `serde` / `serde_json` - Serialization
- `image` 0.24 - Image processing
- `tracing` / `tracing-subscriber` - Logging

## Common Patterns

### Frontend-Backend Communication
```typescript
// Frontend: invoke Tauri command
const settings = await invoke<AppSettings>('load_settings')
await invoke('save_settings', { settings })
```

```rust
// Backend: register command in lib.rs
.invoke_handler(tauri::generate_handler![
    commands::save_settings,
    commands::load_settings,
])
```

### Async Database Access
```rust
let db_state = app.try_state::<Mutex<DatabaseState>>()
    .ok_or("Database not initialized")?;
let db_guard = db_state.lock().await;
let pool = &db_guard.pool;

sqlx::query("SELECT * FROM clipboard_history")
    .fetch_all(pool)
    .await
    .map_err(|e| format!("Query failed: {}", e))?;
```

## Performance Considerations

- Frontend limits in-memory items to 300 (`MAX_MEMORY_ITEMS`)
- Image preview size limit: 5MB
- Memory cleanup interval: 30 minutes
- History cleanup interval: 60 minutes
- Development mode disables incremental compilation for sccache compatibility

## Do NOT

- Suppress TypeScript errors with `as any`, `@ts-ignore`, or `@ts-expect-error`
- Leave empty catch blocks
- Commit without explicit user request
- Use blocking I/O in Tauri commands (use async)
- Modify database schema without migration handling
