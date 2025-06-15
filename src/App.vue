<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { MagnifyingGlassIcon, StarIcon, Cog6ToothIcon } from '@heroicons/vue/24/outline'
import { StarIcon as StarIconSolid } from '@heroicons/vue/24/solid'
import { Tab, TabList, TabGroup, TabPanels, TabPanel } from '@headlessui/vue'
import Settings from './components/Settings.vue'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import Database from '@tauri-apps/plugin-sql'

// 窗口最大化状态
const isMaximized = ref(false)

// 定义设置类型
interface AppSettings {
  max_history_items: number
  max_history_time: number
  hotkey: string
  auto_start: boolean
}

// 保存设置的函数
const saveSettings = async (settings: AppSettings) => {
  try {
    await invoke('save_settings', { settings })
    console.log('Settings saved successfully')
  } catch (error) {
    console.error('Failed to save settings:', error)
    throw error // 让调用者处理错误
  }
}

// 提供给 Settings 组件的方法
const handleSaveSettings = async (settings: AppSettings) => {
  try {
    await saveSettings(settings)
    // 可以在这里添加成功提示
  } catch (error) {
    // 可以在这里添加错误提示
    throw error
  }
}

// Mock data
const clipboardHistory = ref<any[]>([])
const searchQuery = ref('')
const selectedItem = ref(clipboardHistory.value[0])
const showSettings = ref(false)
const selectedTabIndex = ref(0)
const fullImageContent = ref<string | null>(null) // 存储完整图片的 base64 数据
let db: Awaited<ReturnType<any>> | null = null

// 格式化时间显示
const formatTime = (timestamp: string) => {
  const date = new Date(timestamp)
  const now = new Date()
  const diffMs = now.getTime() - date.getTime()
  const diffMins = Math.floor(diffMs / (1000 * 60))
  const diffHours = Math.floor(diffMs / (1000 * 60 * 60))
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24))
  
  if (diffMins < 1) return 'Just now'
  if (diffMins < 60) return `${diffMins}m ago`
  if (diffHours < 24) return `${diffHours}h ago`
  if (diffDays < 7) return `${diffDays}d ago`
  
  // 超过一周显示日期
  return date.toLocaleDateString('en-US', { 
    month: 'short', 
    day: 'numeric',
    ...(date.getFullYear() !== now.getFullYear() ? { year: 'numeric' } : {})
  })
}

// 搜索框引用
const searchInputRef = ref<HTMLInputElement | null>(null)
// 存储Tauri事件监听器的unlisten函数
const unlistenFocus = ref<(() => void) | null>(null)

// 清理搜索框并选中第一个条目的函数
const resetToDefault = async () => {
  // 清理搜索框内容
  searchQuery.value = ''
  
  // 等待下一个tick以确保过滤后的历史列表已更新
  await nextTick()
  
  // 选中第一个条目（如果存在）
  if (filteredHistory.value.length > 0) {
    selectedItem.value = filteredHistory.value[0]
    console.log('Selected first item:', selectedItem.value.id)
    
    // 滚动到选中的条目
    await scrollToSelectedItem(selectedItem.value.id)
  } else {
    selectedItem.value = null
    console.log('No items available to select')
  }
}

// 自动聚焦搜索框
const focusSearchInput = async () => {
  await nextTick()
  if (searchInputRef.value) {
    searchInputRef.value.focus()
    console.log('Search input focused')
  }
}

// 处理窗口焦点事件，当窗口显示/获得焦点时重置状态
const handleWindowFocus = async () => {
  console.log('Window focused, resetting to default state')
  await resetToDefault()
  await focusSearchInput()
}

// 隐藏应用窗口
const hideWindow = async () => {
  try {
    const appWindow = getCurrentWindow()
    await appWindow.hide()
    console.log('Window hidden via Esc key')
  } catch (error) {
    console.error('Failed to hide window:', error)
  }
}

// 最小化窗口
const minimizeWindow = async () => {
  try {
    const appWindow = getCurrentWindow()
    await appWindow.minimize()
    console.log('Window minimized')
  } catch (error) {
    console.error('Failed to minimize window:', error)
  }
}

// 切换最大化状态
const toggleMaximize = async () => {
  try {
    const appWindow = getCurrentWindow()
    if (isMaximized.value) {
      await appWindow.unmaximize()
      isMaximized.value = false
    } else {
      await appWindow.maximize()
      isMaximized.value = true
    }
    console.log('Window maximized state:', isMaximized.value)
  } catch (error) {
    console.error('Failed to toggle maximize:', error)
  }
}

// 检查窗口是否最大化
const checkMaximizedState = async () => {
  try {
    const appWindow = getCurrentWindow()
    isMaximized.value = await appWindow.isMaximized()
  } catch (error) {
    console.error('Failed to check maximized state:', error)
  }
}

// 滚动到选中的条目
const scrollToSelectedItem = async (itemId: number) => {
  await nextTick()
  const selectedElement = document.querySelector(`[data-item-id="${itemId}"]`)
  if (selectedElement) {
    selectedElement.scrollIntoView({
      behavior: 'smooth',
      block: 'nearest',
      inline: 'nearest'
    })
    console.log('Scrolled to selected item:', itemId)
  }
}

const filteredHistory = computed(() => {
  console.log('Computing filteredHistory, selectedTabIndex:', selectedTabIndex.value)
  console.log('Current clipboardHistory:', clipboardHistory.value.map(item => ({ id: item.id, isFavorite: item.isFavorite })))
  
  const query = searchQuery.value.toLowerCase()
  
  // 根据标签页筛选：All显示所有，Favorites只显示收藏的
  const items = selectedTabIndex.value === 0 
    ? clipboardHistory.value 
    : clipboardHistory.value.filter(item => item.isFavorite === true)
  
  console.log('Filtered items count:', items.length)
  
  // 应用搜索过滤
  const result = items.filter(item => 
    item.content.toLowerCase().includes(query)
  )
  
  return result
})

const toggleFavorite = async (item: any) => {
  try {
    console.log('Toggling favorite for item:', item.id, 'Current status:', item.isFavorite, 'Current tab:', selectedTabIndex.value)
    const newFavoriteStatus = !item.isFavorite
    
    // 更新数据库
    await db.execute(
      `UPDATE clipboard_history SET is_favorite = ? WHERE id = ?`,
      [newFavoriteStatus ? 1 : 0, item.id]
    )
    console.log('Database updated successfully')
    
    // 更新内存中的状态
    const index = clipboardHistory.value.findIndex(i => i.id === item.id)
    if (index !== -1) {
      // 强制触发响应式更新
      clipboardHistory.value = clipboardHistory.value.map((historyItem, idx) => {
        if (idx === index) {
          return { ...historyItem, isFavorite: newFavoriteStatus }
        }
        return historyItem
      })
      console.log('Memory state updated, new favorite status:', newFavoriteStatus)
      
      // 如果在收藏夹标签页取消收藏
      if (selectedTabIndex.value === 1 && !newFavoriteStatus) {
        // 如果当前选中的是被取消收藏的项，清除选中状态
        if (selectedItem.value?.id === item.id) {
          selectedItem.value = null
        }
        // 强制重新计算过滤后的列表
        nextTick(() => {
          console.log('Recomputing filtered list after unfavorite in Favorites tab')
        })
      }
    }
  } catch (error) {
    console.error('Failed to toggle favorite:', error)
  }
}

// 检查是否是重复内容，如果是则返回已有条目的ID
const checkDuplicateContent = (content: string): number | null => {
  // 在当前历史记录中查找相同内容的条目
  // 对于图片，使用 imagePath 进行比较；对于文本，使用 content 进行比较
  const existingItem = clipboardHistory.value.find(item => {
    if (item.type === 'image' && item.imagePath) {
      return item.imagePath === content
    }
    return item.content === content
  })
  return existingItem ? existingItem.id : null
}

// 将已有条目移动到最前面并更新时间戳
const moveItemToFront = async (itemId: number) => {
  try {
    const newTimestamp = new Date().toISOString()
    
    // 更新数据库中的时间戳
    await db.execute(
      `UPDATE clipboard_history SET timestamp = ? WHERE id = ?`,
      [newTimestamp, itemId]
    )
    console.log('Database timestamp updated for item:', itemId)
    
    // 在内存中找到该条目
    const itemIndex = clipboardHistory.value.findIndex(item => item.id === itemId)
    if (itemIndex !== -1) {
      // 取出该条目并更新时间戳
      const item = { ...clipboardHistory.value[itemIndex], timestamp: newTimestamp }
      
      // 从原位置移除
      clipboardHistory.value.splice(itemIndex, 1)
      
      // 添加到最前面
      clipboardHistory.value.unshift(item)
      
      console.log('Item moved to front:', itemId, 'new timestamp:', newTimestamp)
    }
  } catch (error) {
    console.error('Failed to move item to front:', error)
  }
}

// 粘贴内容到系统剪贴板
const pasteToClipboard = async (item: any) => {
  if (!item) return
  
  try {
    console.log('Pasting item to clipboard:', item.type, item.id)
    
    // 先隐藏窗口，让焦点回到之前的应用
    const appWindow = getCurrentWindow()
    await appWindow.hide()
    console.log('Window hidden before paste')
    
    // 等待一小段时间确保焦点已经切换
    await new Promise(resolve => setTimeout(resolve, 50))
    
    // 然后执行粘贴操作（包含复制到剪贴板和自动粘贴）
    // 对于图片，如果有完整图片内容，使用完整内容，否则使用缩略图
    const contentToPaste = item.type === 'image' && fullImageContent.value 
      ? fullImageContent.value 
      : item.content
    
    await invoke('paste_to_clipboard', {
      content: contentToPaste,
      contentType: item.type
    })
    console.log('Successfully pasted to clipboard and auto-pasted')
    
  } catch (error) {
    console.error('Failed to paste to clipboard:', error)
    // 如果出错，重新显示窗口
    try {
      const appWindow = getCurrentWindow()
      await appWindow.show()
    } catch (showError) {
      console.error('Failed to show window after error:', showError)
    }
  }
}

const handleKeyDown = (e: KeyboardEvent) => {
  // 防止 Alt 键触发系统菜单
  if (e.altKey) {
    e.preventDefault()
    return
  }

  // 处理Esc键隐藏窗口
  if (e.key === 'Escape') {
    e.preventDefault()
    hideWindow()
    return
  }

  // 处理标签页切换（左右箭头键）
  if (e.key === 'ArrowLeft') {
    e.preventDefault()
    handleTabChange(0)
    return
  } else if (e.key === 'ArrowRight') {
    e.preventDefault()
    handleTabChange(1)
    return
  }

  // 如果没有历史记录，只处理标签页切换
  if (!filteredHistory.value.length) return

  const currentIndex = filteredHistory.value.findIndex(item => item.id === selectedItem.value?.id)
  let newIndex = currentIndex

  if (e.key === 'ArrowUp') {
    e.preventDefault()
    newIndex = currentIndex > 0 ? currentIndex - 1 : filteredHistory.value.length - 1
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    newIndex = currentIndex < filteredHistory.value.length - 1 ? currentIndex + 1 : 0
  } else if (e.key === 'Enter') {
    e.preventDefault()
    // 按Enter键粘贴当前选中的项目
    if (selectedItem.value) {
      pasteToClipboard(selectedItem.value)
    }
    return
  }

  if (newIndex !== currentIndex) {
    selectedItem.value = filteredHistory.value[newIndex]
    // 滚动到新选中的条目
    if (selectedItem.value) {
      scrollToSelectedItem(selectedItem.value.id)
    }
  }
}

// 处理双击事件
const handleDoubleClick = (item: any) => {
  pasteToClipboard(item)
}

const handleTabChange = (index: number) => {
  console.log('Tab changed to:', index)
  selectedTabIndex.value = index
  // 重置搜索和选中状态
  searchQuery.value = ''
  selectedItem.value = null
  
  // 切换标签页后自动聚焦搜索框
  focusSearchInput()
}

onMounted(async () => {
  try {
    const dbPath = 'sqlite:clipboard.db'
    console.log('Connecting to database:', dbPath)
    db = await Database.load(dbPath)
    
    // 读取历史数据
    const rows = await db.select(
      `SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon 
       FROM clipboard_history 
       ORDER BY id DESC`
    )
    clipboardHistory.value = rows.map((row: any) => ({
      id: row.id,
      content: row.content,
      type: row.type,
      timestamp: row.timestamp,
      isFavorite: row.is_favorite === 1,
      imagePath: row.image_path ?? null,
      sourceAppName: row.source_app_name ?? 'Unknown',
      sourceAppIcon: row.source_app_icon ?? null
    }))

    listen<string>('clipboard-text', async (event) => {
      try {
        // 解析事件数据
        const eventData = JSON.parse(event.payload)
        const content = eventData.content
        const sourceAppName = eventData.source_app_name || 'Unknown'
        const sourceAppIcon = eventData.source_app_icon || null
        
        // 检查是否是重复内容
        const duplicateItemId = checkDuplicateContent(content)
        if (duplicateItemId) {
          console.log('Duplicate text content detected, moving item to front:', duplicateItemId)
          await moveItemToFront(duplicateItemId)
          return
        }

        const item = {
          content: content,
          type: 'text',
          timestamp: new Date().toISOString(),
          isFavorite: false,
          imagePath: null,
          sourceAppName: sourceAppName,
          sourceAppIcon: sourceAppIcon
        }
        // 插入新记录到数据库
        await db.execute(
          `INSERT INTO clipboard_history (content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon) 
           VALUES (?, ?, ?, ?, ?, ?, ?)`,
          [item.content, item.type, item.timestamp, 0, item.imagePath, item.sourceAppName, item.sourceAppIcon]
        )
        const rows = await db.select(`SELECT last_insert_rowid() as id`)
        const id = rows[0]?.id || Date.now()
        clipboardHistory.value.unshift(Object.assign({ id }, item))
      } catch (error) {
        console.error('Failed to process clipboard text:', error)
      }
    })

    listen<string>('clipboard-image', async (event) => {
      try {
        // 解析事件数据
        const eventData = JSON.parse(event.payload)
        const imagePath = eventData.path
        const thumbnail = eventData.thumbnail
        const sourceAppName = eventData.source_app_name || 'Unknown'
        const sourceAppIcon = eventData.source_app_icon || null
        
        // 检查是否是重复内容（使用文件路径作为内容标识）
        const duplicateItemId = checkDuplicateContent(imagePath)
        if (duplicateItemId) {
          console.log('Duplicate image content detected, moving item to front:', duplicateItemId)
          await moveItemToFront(duplicateItemId)
          return
        }

        const item = {
          content: thumbnail, // 使用缩略图用于列表显示
          type: 'image',
          timestamp: new Date().toISOString(),
          isFavorite: false,
          imagePath: imagePath, // 存储完整图片的路径
          sourceAppName: sourceAppName,
          sourceAppIcon: sourceAppIcon
        }
        
        // 插入新记录到数据库
        await db.execute(
          `INSERT INTO clipboard_history (content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon) 
           VALUES (?, ?, ?, ?, ?, ?, ?)`,
          [item.content, item.type, item.timestamp, 0, item.imagePath, item.sourceAppName, item.sourceAppIcon]
        )
        const rows = await db.select(`SELECT last_insert_rowid() as id`)
        const id = rows[0]?.id || Date.now()
        clipboardHistory.value.unshift(Object.assign({ id }, item))
      } catch (error) {
        console.error('Failed to process clipboard image:', error)
      }
    })

    window.addEventListener('keydown', handleKeyDown)
    
    // 处理窗口关闭事件，隐藏到托盘而不是关闭
    const appWindow = getCurrentWindow()
    
    // 监听窗口焦点事件
    const unlistenFocusFunc = await appWindow.onFocusChanged(({ payload: focused }) => {
      if (focused) {
        console.log('Window focused via Tauri API, resetting to default state')
        handleWindowFocus()
      }
    })
    
    // 将unlisten函数存储到ref中
    unlistenFocus.value = unlistenFocusFunc
    
    await appWindow.onCloseRequested(async (event) => {
      // 阻止默认的关闭行为
      event.preventDefault()
      // 隐藏窗口到系统托盘
      await appWindow.hide()
      console.log('Window hidden to system tray')
    })
    
    // 组件挂载后自动聚焦搜索框
    await focusSearchInput()
    
    // 检查初始最大化状态
    await checkMaximizedState()
    
    // 监听窗口大小变化事件
    const unlistenResize = await appWindow.listen('tauri://resize', async () => {
      await checkMaximizedState()
    })
    
    // 存储 unlisten 函数以便清理
    onUnmounted(() => {
      unlistenResize()
    })
  } catch (error) {
    console.error('Database error:', error)
  }
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeyDown)
  
  // 清理Tauri窗口焦点事件监听器
  if (unlistenFocus.value) {
    unlistenFocus.value()
  }
})

// 监听标签页变化
watch(selectedTabIndex, () => {
  // 切换标签页时重置搜索
  searchQuery.value = ''
  // 重置选中项
  selectedItem.value = null
  // 清除完整图片内容
  fullImageContent.value = null
})

// 监听选中项变化，当选中图片时加载完整图片
watch(selectedItem, async (newItem) => {
  if (newItem && newItem.type === 'image' && newItem.imagePath) {
    try {
      console.log('Loading full image from path:', newItem.imagePath)
      const fullImage = await invoke('load_image_file', { imagePath: newItem.imagePath }) as string
      fullImageContent.value = fullImage
    } catch (error) {
      console.error('Failed to load full image:', error)
      // 如果加载失败，使用缩略图作为后备
      fullImageContent.value = newItem.content
    }
  } else {
    fullImageContent.value = null
  }
})

// 开发者工具函数（生产环境已注释，开发时可取消注释）
const openDevTools = () => {
  // 使用快捷键打开开发者工具
  document.dispatchEvent(new KeyboardEvent('keydown', {
    key: 'i',
    code: 'KeyI',
    ctrlKey: true,
    shiftKey: true,
    keyCode: 73,
    which: 73,
    bubbles: true
  }))
}

// 重置数据库函数（仅用于开发环境修复迁移冲突）
const resetDatabase = async () => {
  if (confirm('确定要重置数据库吗？这将删除所有剪贴板历史记录！')) {
    try {
      await invoke('reset_database')
      console.log('数据库重置成功')
      alert('数据库重置成功！请重启应用程序。')
      // 重新加载页面以重新初始化
      window.location.reload()
    } catch (error) {
      console.error('重置数据库失败:', error)
      alert('重置数据库失败: ' + error)
    }
  }
}
</script>

<template>
  <div class="h-screen flex flex-col bg-gray-50">
    <!-- Custom Title Bar -->
    <div class="bg-white border-b border-gray-200 flex items-center justify-between h-8 select-none" data-tauri-drag-region>
      <div class="flex items-center px-3" data-tauri-drag-region>
        <span class="text-xs text-gray-600">Clipboard Manager</span>
      </div>
      <div class="flex">
        <button 
          @click="minimizeWindow"
          class="w-10 h-8 hover:bg-gray-100 flex items-center justify-center transition-colors"
          title="Minimize"
        >
          <svg class="w-3 h-3 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20 12H4"></path>
          </svg>
        </button>
        <button 
          @click="toggleMaximize"
          class="w-10 h-8 hover:bg-gray-100 flex items-center justify-center transition-colors"
          :title="isMaximized ? 'Restore' : 'Maximize'"
        >
          <!-- 最大化图标 -->
          <svg v-if="!isMaximized" class="w-3 h-3 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <rect x="5" y="5" width="14" height="14" stroke-width="2" rx="1"></rect>
          </svg>
          <!-- 还原图标 -->
          <svg v-else class="w-3 h-3 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-width="2" d="M8 8h8v8H8z M12 4h8v8h-8z"></path>
          </svg>
        </button>
        <button 
          @click="hideWindow"
          class="w-10 h-8 hover:bg-red-100 hover:text-red-600 flex items-center justify-center transition-colors"
          title="Close"
        >
          <svg class="w-3 h-3 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
          </svg>
        </button>
      </div>
    </div>

    <!-- Header -->
    <header class="bg-white border-b border-gray-200 px-6 py-4 flex-shrink-0 shadow-sm">
      <div class="flex items-center justify-between">
        <div class="flex items-center space-x-3">
          <div class="w-8 h-8 bg-gradient-to-br from-blue-500 to-purple-600 rounded-lg flex items-center justify-center">
            <svg class="w-5 h-5 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"></path>
            </svg>
          </div>
          <h1 class="text-xl font-semibold text-gray-900">Clipboard Manager</h1>
        </div>
        <div class="flex items-center space-x-3">
          <!-- 开发者工具按钮（生产环境已注释，开发时可取消注释） -->
          <button 
            class="px-3 py-2 text-sm font-medium text-gray-600 hover:text-gray-900 hover:bg-gray-100 rounded-lg transition-colors duration-200"
            @click="openDevTools"
          >
            Dev Tools
          </button>
          <button 
            class="px-3 py-2 text-sm font-medium text-red-600 hover:text-red-900 hover:bg-red-100 rounded-lg transition-colors duration-200"
            @click="resetDatabase"
            title="重置数据库（修复迁移冲突）"
          >
            Reset DB
          </button>
          <button 
            class="p-2 text-gray-600 hover:text-gray-900 hover:bg-gray-100 rounded-lg transition-colors duration-200"
            @click="showSettings = !showSettings"
          >
            <Cog6ToothIcon class="w-5 h-5" />
          </button>
        </div>
      </div>
    </header>

    <!-- Main Content -->
    <div class="flex-1 flex min-h-0">
      <!-- Left Sidebar -->
      <div class="w-80 lg:w-96 bg-white border-r border-gray-200 flex flex-col min-h-0 shadow-sm">
        <!-- Tabs -->
        <TabGroup v-model="selectedTabIndex" as="div" class="flex flex-col h-full" @change="handleTabChange">
          <div class="border-b border-gray-200 flex-shrink-0 bg-gray-50">
            <TabList class="flex">
              <!-- All 标签页 -->
              <Tab v-slot="{ selected }" as="template">
                <button
                  class="flex-1 px-4 py-2.5 text-xs font-medium border-b-2 -mb-px transition-all duration-200"
                  :class="[
                    selected
                      ? 'text-blue-600 border-blue-600 bg-white'
                      : 'text-gray-500 border-transparent hover:text-gray-700 hover:border-gray-300 bg-gray-50'
                  ]"
                >
                  <span class="flex items-center space-x-1.5">
                    <svg class="w-3.5 h-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"></path>
                    </svg>
                    <span>All</span>
                  </span>
                </button>
              </Tab>
              <!-- Favorites 标签页 -->
              <Tab v-slot="{ selected }" as="template">
                <button
                  class="flex-1 px-4 py-2.5 text-xs font-medium border-b-2 -mb-px transition-all duration-200"
                  :class="[
                    selected
                      ? 'text-blue-600 border-blue-600 bg-white'
                      : 'text-gray-500 border-transparent hover:text-gray-700 hover:border-gray-300 bg-gray-50'
                  ]"
                >
                  <span class="flex items-center space-x-1.5">
                    <StarIcon class="w-3.5 h-3.5" />
                    <span>Favorites</span>
                  </span>
                </button>
              </Tab>
            </TabList>
          </div>

          <TabPanels class="flex-1 min-h-0">
            <TabPanel class="h-full flex flex-col min-h-0">
              <!-- Search -->
              <div class="p-3 border-b border-gray-100 flex-shrink-0">
                <div class="relative">
                  <input
                    v-model="searchQuery"
                    type="text"
                    placeholder="Search clipboard history..."
                    class="w-full pl-8 pr-3 py-2 border border-gray-200 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all duration-200 text-xs"
                    ref="searchInputRef"
                  />
                  <div class="absolute inset-y-0 left-0 pl-2.5 flex items-center pointer-events-none">
                    <MagnifyingGlassIcon class="h-3.5 w-3.5 text-gray-400" />
                  </div>
                </div>
              </div>

              <!-- History List -->
              <div class="flex-1 overflow-y-auto min-h-0">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  :data-item-id="item.id"
                  class="group px-3 py-2 border-b border-gray-50 hover:bg-blue-50 cursor-pointer transition-all duration-200"
                  :class="{ 
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id,
                    'hover:bg-gray-50': selectedItem?.id !== item.id
                  }"
                  @click="selectedItem = item"
                  @dblclick="handleDoubleClick(item)"
                >
                  <div class="flex items-start justify-between">
                    <div class="flex items-start space-x-2 flex-1 min-w-0 mr-2">
                      <!-- 源应用图标 -->
                      <div class="flex-shrink-0 w-6 h-6 mt-0.5">
                        <img 
                          v-if="item.sourceAppIcon" 
                          :src="item.sourceAppIcon" 
                          :alt="item.sourceAppName"
                          class="w-6 h-6 rounded object-contain"
                        />
                        <div 
                          v-else 
                          class="w-6 h-6 rounded bg-gray-200 flex items-center justify-center"
                          :title="item.sourceAppName"
                        >
                          <svg class="w-3 h-3 text-gray-500" fill="currentColor" viewBox="0 0 20 20">
                            <path fill-rule="evenodd" d="M3 4a1 1 0 011-1h12a1 1 0 011 1v2a1 1 0 01-1 1H4a1 1 0 01-1-1V4zm0 4a1 1 0 011-1h12a1 1 0 011 1v6a1 1 0 01-1 1H4a1 1 0 01-1-1V8zm8 2a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd"></path>
                          </svg>
                        </div>
                      </div>
                      
                      <div class="flex-1 min-w-0">
                        <div class="flex items-center justify-between mb-1">
                          <div class="flex items-center space-x-1">
                            <div 
                              class="w-1.5 h-1.5 rounded-full"
                              :class="item.type === 'text' ? 'bg-green-400' : 'bg-purple-400'"
                            ></div>
                            <span class="text-xs font-medium text-gray-500 uppercase tracking-wide">
                              {{ item.type }}
                            </span>
                            <span class="text-xs text-gray-400">
                              · {{ item.sourceAppName }}
                            </span>
                          </div>
                          <span class="text-xs text-gray-400">
                            {{ formatTime(item.timestamp) }}
                          </span>
                        </div>
                        <p class="text-xs text-gray-900 line-clamp-2 leading-snug">
                          {{ item.type === 'text' ? item.content : 'Image content' }}
                        </p>
                      </div>
                    </div>
                    <button
                      class="flex-shrink-0 p-0.5 text-gray-400 hover:text-yellow-500 transition-colors duration-200 opacity-0 group-hover:opacity-100"
                      :class="{ 'opacity-100': item.isFavorite }"
                      @click.stop="toggleFavorite(item)"
                    >
                      <StarIcon v-if="!item.isFavorite" class="w-3.5 h-3.5" />
                      <StarIconSolid v-else class="w-3.5 h-3.5 text-yellow-500" />
                    </button>
                  </div>
                </div>
                
                <!-- Empty state -->
                <div v-if="filteredHistory.length === 0" class="flex flex-col items-center justify-center py-8 px-3">
                  <div class="w-12 h-12 bg-gray-100 rounded-full flex items-center justify-center mb-3">
                    <MagnifyingGlassIcon class="w-6 h-6 text-gray-400" />
                  </div>
                  <p class="text-gray-500 text-xs text-center">
                    {{ searchQuery ? 'No items match your search' : 'No clipboard history yet' }}
                  </p>
                </div>
              </div>
            </TabPanel>

            <TabPanel class="h-full flex flex-col min-h-0">
              <!-- Search -->
              <div class="p-3 border-b border-gray-100 flex-shrink-0">
                <div class="relative">
                  <input
                    v-model="searchQuery"
                    type="text"
                    placeholder="Search favorites..."
                    class="w-full pl-8 pr-3 py-2 border border-gray-200 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all duration-200 text-xs"
                    ref="searchInputRef"
                  />
                  <div class="absolute inset-y-0 left-0 pl-2.5 flex items-center pointer-events-none">
                    <MagnifyingGlassIcon class="h-3.5 w-3.5 text-gray-400" />
                  </div>
                </div>
              </div>

              <!-- Favorites List -->
              <div class="flex-1 overflow-y-auto min-h-0">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  :data-item-id="item.id"
                  class="group px-3 py-2 border-b border-gray-50 hover:bg-blue-50 cursor-pointer transition-all duration-200"
                  :class="{ 
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id,
                    'hover:bg-gray-50': selectedItem?.id !== item.id
                  }"
                  @click="selectedItem = item"
                  @dblclick="handleDoubleClick(item)"
                >
                  <div class="flex items-start justify-between">
                    <div class="flex items-start space-x-2 flex-1 min-w-0 mr-2">
                      <!-- 源应用图标 -->
                      <div class="flex-shrink-0 w-6 h-6 mt-0.5">
                        <img 
                          v-if="item.sourceAppIcon" 
                          :src="item.sourceAppIcon" 
                          :alt="item.sourceAppName"
                          class="w-6 h-6 rounded object-contain"
                        />
                        <div 
                          v-else 
                          class="w-6 h-6 rounded bg-gray-200 flex items-center justify-center"
                          :title="item.sourceAppName"
                        >
                          <svg class="w-3 h-3 text-gray-500" fill="currentColor" viewBox="0 0 20 20">
                            <path fill-rule="evenodd" d="M3 4a1 1 0 011-1h12a1 1 0 011 1v2a1 1 0 01-1 1H4a1 1 0 01-1-1V4zm0 4a1 1 0 011-1h12a1 1 0 011 1v6a1 1 0 01-1 1H4a1 1 0 01-1-1V8zm8 2a1 1 0 100-2 1 1 0 000 2z" clip-rule="evenodd"></path>
                          </svg>
                        </div>
                      </div>
                      
                      <div class="flex-1 min-w-0">
                        <div class="flex items-center justify-between mb-1">
                          <div class="flex items-center space-x-1">
                            <div 
                              class="w-1.5 h-1.5 rounded-full"
                              :class="item.type === 'text' ? 'bg-green-400' : 'bg-purple-400'"
                            ></div>
                            <span class="text-xs font-medium text-gray-500 uppercase tracking-wide">
                              {{ item.type }}
                            </span>
                            <span class="text-xs text-gray-400">
                              · {{ item.sourceAppName }}
                            </span>
                          </div>
                          <span class="text-xs text-gray-400">
                            {{ formatTime(item.timestamp) }}
                          </span>
                        </div>
                        <p class="text-xs text-gray-900 line-clamp-2 leading-snug">
                          {{ item.type === 'text' ? item.content : 'Image content' }}
                        </p>
                      </div>
                    </div>
                    <button
                      class="flex-shrink-0 p-0.5 text-yellow-500 hover:text-gray-400 transition-colors duration-200"
                      @click.stop="toggleFavorite(item)"
                    >
                      <StarIconSolid class="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
                
                <!-- Empty state for favorites -->
                <div v-if="filteredHistory.length === 0" class="flex flex-col items-center justify-center py-8 px-3">
                  <div class="w-12 h-12 bg-gray-100 rounded-full flex items-center justify-center mb-3">
                    <StarIcon class="w-6 h-6 text-gray-400" />
                  </div>
                  <p class="text-gray-500 text-xs text-center">
                    {{ searchQuery ? 'No favorites match your search' : 'No favorites yet' }}
                  </p>
                  <p class="text-gray-400 text-xs text-center mt-1">
                    Click the star icon to add items to favorites
                  </p>
                </div>
              </div>
            </TabPanel>
          </TabPanels>
        </TabGroup>
      </div>

      <!-- Right Content -->
      <div class="flex-1 flex flex-col min-h-0 bg-white">
        <div class="px-4 py-3 border-b border-gray-200 flex-shrink-0">
          <div class="flex items-center justify-between">
            <div class="flex items-center space-x-2">
              <div 
                v-if="selectedItem"
                class="w-2.5 h-2.5 rounded-full"
                :class="selectedItem.type === 'text' ? 'bg-green-400' : 'bg-purple-400'"
              ></div>
              <h2 class="text-base font-semibold text-gray-900">
                {{ selectedItem?.type === 'text' ? 'Text Content' : selectedItem?.type === 'image' ? 'Image Preview' : 'Select an Item' }}
              </h2>
            </div>
            <span class="text-xs text-gray-500" v-if="selectedItem">
              {{ formatTime(selectedItem.timestamp) }}
            </span>
          </div>
        </div>
        
        <div class="flex-1 p-4 overflow-y-auto min-h-0">
          <div v-if="selectedItem" class="h-full">
            <div class="bg-gray-50 rounded-lg border border-gray-200 p-4 min-h-full">
              <template v-if="selectedItem.type === 'text'">
                <div class="prose prose-sm max-w-none">
                  <pre class="whitespace-pre-wrap break-words text-gray-900 font-mono text-xs leading-normal">{{ selectedItem.content }}</pre>
                </div>
              </template>
              <template v-else>
                <div class="flex items-center justify-center">
                  <div v-if="!fullImageContent" class="flex flex-col items-center justify-center py-8">
                    <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-blue-600 mb-4"></div>
                    <p class="text-gray-500 text-sm">Loading full image...</p>
                  </div>
                  <img
                    v-else
                    :src="fullImageContent"
                    alt="Clipboard image"
                    class="max-w-full max-h-full object-contain rounded-lg shadow-lg"
                  />
                </div>
              </template>
            </div>
          </div>
          <div v-else class="h-full flex flex-col items-center justify-center text-gray-400">
            <div class="w-16 h-16 bg-gray-100 rounded-full flex items-center justify-center mb-3">
              <svg class="w-8 h-8" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M15 15l-2 5L9 9l11 4-5 2zm0 0l5 5M7.188 2.239l.777 2.897M5.136 7.965l-2.898-.777M13.95 4.05l-2.122 2.122m-5.657 5.656l-2.12 2.122"></path>
              </svg>
            </div>
            <p class="text-base font-medium mb-2">Select an item to preview</p>
            <p class="text-xs text-center max-w-sm">
              Choose any item from the clipboard history to see its content here. 
              Double-click or press Enter to paste it.
            </p>
          </div>
        </div>
      </div>
    </div>

    <!-- Settings Modal -->
    <Settings v-model:show="showSettings" @save-settings="handleSaveSettings" />
  </div>
</template>

<style>
/* 自定义标题栏样式 */
[data-tauri-drag-region] {
  -webkit-app-region: drag;
  user-select: none;
}

[data-tauri-drag-region] button {
  -webkit-app-region: no-drag;
}

/* 确保滚动条样式统一 */
::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: #d1d5db;
  border-radius: 3px;
}

::-webkit-scrollbar-thumb:hover {
  background: #9ca3af;
}

/* 文本截断样式 */
.line-clamp-2 {
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* 平滑过渡效果 */
.transition-all {
  transition-property: all;
  transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
}

/* 渐变背景 */
.bg-gradient-to-br {
  background-image: linear-gradient(to bottom right, var(--tw-gradient-stops));
}

/* 确保图标大小正确 */
.heroicon {
  width: 1.5rem;
  height: 1.5rem;
}

/* 改进的焦点样式 */
input:focus {
  box-shadow: 0 0 0 3px rgba(59, 130, 246, 0.1);
}

/* 按钮悬停效果 */
button:hover {
  transform: translateY(-1px);
}

/* 卡片阴影效果 */
.shadow-sm {
  box-shadow: 0 1px 2px 0 rgba(0, 0, 0, 0.05);
}

.shadow-lg {
  box-shadow: 0 10px 15px -3px rgba(0, 0, 0, 0.1), 0 4px 6px -2px rgba(0, 0, 0, 0.05);
}

/* 改进的选中状态 */
.bg-blue-100 {
  background-color: rgb(219 234 254);
}

.border-blue-200 {
  border-color: rgb(191 219 254);
}

/* 空状态样式 */
.empty-state {
  opacity: 0.6;
}

/* 响应式字体和布局 */
@media (max-width: 768px) {
  .text-xl {
    font-size: 1.125rem;
  }
}

/* 超紧凑模式 */
@media (max-width: 1024px) {
  .w-80 {
    width: 18rem;
  }
}

/* 文本行数限制 */
.line-clamp-1 {
  display: -webkit-box;
  -webkit-line-clamp: 1;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

/* 确保小字体清晰度 */
.text-xs {
  font-size: 0.75rem;
  line-height: 1rem;
}

/* 更紧凑的行高 */
.leading-snug {
  line-height: 1.375;
}

/* 优化图标渲染质量 */
img[alt*="source"] {
  image-rendering: -webkit-optimize-contrast;
  image-rendering: crisp-edges;
  image-rendering: pixelated;
}

/* 为源应用图标优化渲染 */
.source-app-icon {
  image-rendering: -webkit-optimize-contrast;
  image-rendering: crisp-edges;
  image-rendering: pixelated;
}

/* 优化所有源应用图标的显示 */
img[alt$="sourceAppName"] {
  image-rendering: -webkit-optimize-contrast;
  image-rendering: crisp-edges;
  image-rendering: auto;
  filter: contrast(1.1) brightness(1.05);
  width: 32px !important;
  height: 32px !important;
  max-width: 32px;
  max-height: 32px;
}

/* 更新图标容器尺寸 */
.source-icon-container {
  width: 32px !important;
  height: 32px !important;
  flex-shrink: 0;
}
</style>

<style scoped>
.logo.vite:hover {
  filter: drop-shadow(0 0 2em #747bff);
}

.logo.vue:hover {
  filter: drop-shadow(0 0 2em #249b73);
}

</style>
<style>
:root {
  font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 24px;
  font-weight: 400;

  color: #0f0f0f;
  background-color: #f6f6f6;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

.container {
  margin: 0;
  padding-top: 10vh;
  display: flex;
  flex-direction: column;
  justify-content: center;
  text-align: center;
}

.logo {
  height: 6em;
  padding: 1.5em;
  will-change: filter;
  transition: 0.75s;
}

.logo.tauri:hover {
  filter: drop-shadow(0 0 2em #24c8db);
}

.row {
  display: flex;
  justify-content: center;
}

a {
  font-weight: 500;
  color: #646cff;
  text-decoration: inherit;
}

a:hover {
  color: #535bf2;
}

h1 {
  text-align: center;
}

input,
button {
  border-radius: 8px;
  border: 1px solid transparent;
  padding: 0.6em 1.2em;
  font-size: 1em;
  font-weight: 500;
  font-family: inherit;
  color: #0f0f0f;
  background-color: #ffffff;
  transition: border-color 0.25s;
  box-shadow: 0 2px 2px rgba(0, 0, 0, 0.2);
}

button {
  cursor: pointer;
}

button:hover {
  border-color: #396cd8;
}
button:active {
  border-color: #396cd8;
  background-color: #e8e8e8;
}

input,
button {
  outline: none;
}

#greet-input {
  margin-right: 5px;
}

@media (prefers-color-scheme: dark) {
  :root {
    color: #f6f6f6;
    background-color: #2f2f2f;
  }

  a:hover {
    color: #24c8db;
  }

  input,
  button {
    color: #ffffff;
    background-color: #0f0f0f98;
  }
  button:active {
    background-color: #0f0f0f69;
  }
}

</style>