# Clipboard Manager

## Project Overview

**Clipboard Manager** is a powerful, cross-platform clipboard history tool built with **Tauri v2**. It is designed to be lightweight, fast, and privacy-focused (all data stored locally).

**Key Features:**
*   **Clipboard History:** Tracks both text and image content.
*   **Smart Paste:** One-click paste back to the previously active application.
*   **Grouping & Favorites:** Organize clippings into groups or mark them as favorites.
*   **Search:** Fast local search through history.
*   **Platform Integration:** 
    *   **Windows:** Standard system tray and global shortcuts.
    *   **macOS:** Experimental support for "NSPanel" to allow the window to float over full-screen apps.
*   **Privacy:** Uses a local SQLite database (`clipboard.db`).

## Tech Stack

*   **Frontend:**
    *   **Framework:** Vue 3 (Composition API)
    *   **Language:** TypeScript
    *   **Build Tool:** Vite
    *   **Styling:** TailwindCSS + DaisyUI
    *   **State Management:** Reactive variables / Composables (e.g., `useImageCache`, `useToast`)

*   **Backend:**
    *   **Core:** Rust (Tauri v2)
    *   **Database:** SQLite (via `sqlx` and `tauri-plugin-sql`)
    *   **System Integration:** `enigo` (input simulation), `rdev` (event listening), custom macOS Objective-C bindings.

## Project Structure

*   **`src/`**: Frontend source code.
    *   **`components/`**: UI components (`ConfirmDialog`, `Settings`, `Toast`).
    *   **`composables/`**: Shared logic hooks.
    *   **`views/`**: Main application views (`Log.vue` is likely the main history view).
    *   **`App.vue`**: Root component.
    *   **`main.ts`**: Application entry point.
*   **`src-tauri/`**: Backend source code.
    *   **`src/lib.rs`**: Main entry point for the library, handles setup, database init, and command registration.
    *   **`src/commands.rs`**: Implementation of frontend-callable Tauri commands.
    *   **`src/macos_paste.rs`**: macOS-specific implementation for pasting.
    *   **`tauri.conf.json`**: Tauri configuration (permissions, window settings, bundle config).
    *   **`capabilities/`**: Security capability definitions (permissions).

## Building and Running

**Prerequisites:**
*   Node.js (v16+)
*   Rust (latest stable)
*   OS-specific build tools (Visual Studio C++ Build Tools on Windows, Xcode on macOS)

**Development:**
```bash
# Install frontend dependencies
npm install

# Run in development mode (Frontend + Backend hot reload)
npm run tauri dev

# Fast dev mode (uses sccache if available)
npm run tauri:dev:fast
```

**Production Build:**
```bash
# Build for production
npm run tauri build
```

## Development Conventions

*   **Tauri v2:** This project uses Tauri v2. Ensure you consult the Tauri v2 documentation for API references.
*   **Commands:** Backend functionality is exposed via `#[tauri::command]` in Rust and invoked via `invoke` in the frontend.
*   **Database:** Schema changes should be handled in the `init_database` function in `src-tauri/src/lib.rs`.
*   **Logging:** The application uses `tracing` for backend logging.
*   **Releasing:** Use the provided release scripts (`npm run release:patch`, etc.) to handle versioning and git tagging.

## Key Files
*   `src-tauri/tauri.conf.json`: The source of truth for app configuration.
*   `src-tauri/src/lib.rs`: The "brain" of the backend initialization.
*   `src/views/Log.vue`: Likely the primary UI for interacting with clipboard history.
