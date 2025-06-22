<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { MagnifyingGlassIcon, StarIcon, Cog6ToothIcon } from '@heroicons/vue/24/outline'
import { StarIcon as StarIconSolid } from '@heroicons/vue/24/solid'
import { Tab, TabList, TabGroup, TabPanels, TabPanel } from '@headlessui/vue'
import Settings from './components/Settings.vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import Database from '@tauri-apps/plugin-sql'
import { 
  onTextUpdate, 
  onImageUpdate, 
  startListening
} from 'tauri-plugin-clipboard-api'


// 窗口最大化状态
const isMaximized = ref(false)

// 定义设置类型
interface AppSettings {
  max_history_items: number
  max_history_time: number
  hotkey: string
  auto_start: boolean
}

// 内存中的历史记录限制 - 更严格的限制
const MAX_MEMORY_ITEMS = 100 // 降低从200到100
const MAX_IMAGE_PREVIEW_SIZE = 2 * 1024 * 1024 // 降低从5MB到2MB

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
const isSearching = ref(false) // 添加搜索状态标识
const isLoadingMore = ref(false) // 添加加载更多状态
const hasMoreData = ref(true) // 是否还有更多数据
const currentOffset = ref(0) // 当前加载的偏移量

// 事件监听器清理函数存储
let unlistenClipboardText: (() => void) | null = null
let unlistenClipboardImage: (() => void) | null = null
let unlistenClipboard: (() => Promise<void>) | null = null
let memoryCleanupInterval: ReturnType<typeof setInterval> | null = null

// 防重复机制：记录最近处理的图片
let lastImageProcessTime = 0

// 优化的内存管理函数（更激进的清理策略）
const trimMemoryHistory = () => {
  // 如果不是在搜索状态，且历史记录超过限制，移除最旧的非收藏条目
  if (!searchQuery.value && clipboardHistory.value.length > MAX_MEMORY_ITEMS) {
    const itemsToRemove = clipboardHistory.value.length - MAX_MEMORY_ITEMS
    let removed = 0
    
    // 从后往前遍历（最旧的在后面）
    for (let i = clipboardHistory.value.length - 1; i >= 0 && removed < itemsToRemove; i--) {
      if (!clipboardHistory.value[i].isFavorite) {
        clipboardHistory.value.splice(i, 1)
        removed++
      }
    }
    
    console.log(`内存优化：从显示列表中移除了 ${removed} 条旧记录（数据库中仍保留）`)
  }
  
  // 强制垃圾回收（如果可用）
  if (typeof (window as any).gc === 'function') {
    (window as any).gc()
  }
}

// 优化的时间格式化函数（减少对象创建）
const formatTime = (() => {
  const timeCache = new Map<string, string>()
  const maxCacheSize = 100
  
  const formatFunction = (timestamp: string): string => {
    // 检查缓存
    if (timeCache.has(timestamp)) {
      return timeCache.get(timestamp)!
    }
    
    const date = new Date(timestamp)
    const now = new Date()
    const diffMs = now.getTime() - date.getTime()
    const diffMins = Math.floor(diffMs / (1000 * 60))
    const diffHours = Math.floor(diffMs / (1000 * 60 * 60))
    const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24))
    
    let result: string
    if (diffMins < 1) result = 'Just now'
    else if (diffMins < 60) result = `${diffMins}m ago`
    else if (diffHours < 24) result = `${diffHours}h ago`
    else if (diffDays < 7) result = `${diffDays}d ago`
    else {
      // 超过一周显示日期
      result = date.toLocaleDateString('en-US', { 
        month: 'short', 
        day: 'numeric',
        ...(date.getFullYear() !== now.getFullYear() ? { year: 'numeric' } : {})
      })
    }
    
    // 添加到缓存
    if (timeCache.size >= maxCacheSize) {
      // 清理旧缓存
      const firstKey = timeCache.keys().next().value
      timeCache.delete(firstKey)
    }
    timeCache.set(timestamp, result)
    
    return result
  }
  
  // 添加清理缓存的方法
  ;(formatFunction as any).clearCache = () => {
    timeCache.clear()
    console.log('时间格式化缓存已清理')
  }
  
  return formatFunction as typeof formatFunction & { clearCache: () => void }
})()

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
    // 程序化点击 All 标签
    const allTab = document.querySelector('[role="tablist"] button:first-child') as HTMLButtonElement
    if (allTab) {
      allTab.click()
    }
    return
  } else if (e.key === 'ArrowRight') {
    e.preventDefault()
    // 程序化点击 Favorites 标签
    const favoritesTab = document.querySelector('[role="tablist"] button:last-child') as HTMLButtonElement
    if (favoritesTab) {
      favoritesTab.click()
    }
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
  
  // 重置分页状态
  currentOffset.value = 0
  hasMoreData.value = true
  
  // 重新加载对应标签页的数据
  loadRecentHistory()
  
  // 切换标签页后自动聚焦搜索框
  focusSearchInput()
}

// 监听选中项变化，当选中图片时加载完整图片
watch(selectedItem, async (newItem) => {
  // 清理之前的图片内容，释放内存
  if (fullImageContent.value) {
    fullImageContent.value = null
  }
  
  if (newItem && newItem.type === 'image') {
    try {
      // 使用新插件：图片数据直接存储在content字段中
      if (newItem.imagePath) {
        // 如果有文件路径，尝试从文件加载（兼容旧数据）
        console.log('Loading full image from path:', newItem.imagePath)
        const fullImage = await invoke('load_image_file', { imagePath: newItem.imagePath }) as string
        
        // 检查图片大小，如果过大则不在内存中保存
        if (fullImage.length > MAX_IMAGE_PREVIEW_SIZE) {
          console.warn('完整图片过大，使用缩略图显示')
          fullImageContent.value = newItem.content
        } else {
          fullImageContent.value = fullImage
        }
      } else {
        // 新插件模式：直接使用content中的base64数据
        console.log('Using base64 image data from content field')
        if (newItem.content && typeof newItem.content === 'string') {
          // 检查图片大小
          if (newItem.content.length > MAX_IMAGE_PREVIEW_SIZE) {
            console.warn('图片数据过大，限制显示')
            // 即使过大也显示，因为这是唯一的数据源
            fullImageContent.value = newItem.content
          } else {
            fullImageContent.value = newItem.content
          }
        } else {
          console.warn('图片项目缺少内容数据')
          fullImageContent.value = null
        }
      }
    } catch (error) {
      console.error('Failed to load image:', error)
      // 如果加载失败，尝试使用content作为后备
      fullImageContent.value = (newItem.content && typeof newItem.content === 'string') ? newItem.content : null
    }
  } else {
    fullImageContent.value = null
  }
})

// 添加数据库搜索函数
const searchFromDatabase = async () => {
  if (!db || !searchQuery.value.trim()) {
    return
  }
  
  isSearching.value = true
  
  try {
    const query = searchQuery.value.toLowerCase()
    const isFavoritesTab = selectedTabIndex.value === 1
    
    // 构建SQL查询
    let sql = `
      SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon 
      FROM clipboard_history 
      WHERE LOWER(content) LIKE ?
    `
    
    const params = [`%${query}%`]
    
    // 如果是收藏标签页，只搜索收藏的项目
    if (isFavoritesTab) {
      sql += ' AND is_favorite = 1'
    }
    
    sql += ' ORDER BY timestamp DESC LIMIT 500' // 限制最多返回500条结果
    
    const rows = await db.select(sql, params)
    
    // 将搜索结果转换为前端格式
    const searchResults = rows.map((row: any) => ({
      id: row.id,
      content: row.content,
      type: row.type,
      timestamp: row.timestamp,
      isFavorite: row.is_favorite === 1,
      imagePath: row.image_path ?? null,
      sourceAppName: row.source_app_name ?? 'Unknown',
      sourceAppIcon: row.source_app_icon ?? null
    }))
    
    // 更新内存中的历史记录为搜索结果
    clipboardHistory.value = searchResults
    
    console.log(`数据库搜索完成，找到 ${searchResults.length} 条记录`)
  } catch (error) {
    console.error('数据库搜索失败:', error)
  } finally {
    isSearching.value = false
  }
}

// 添加防抖函数
function debounce<T extends (...args: any[]) => any>(func: T, wait: number): (...args: Parameters<T>) => void {
  let timeout: ReturnType<typeof setTimeout> | null = null
  return function (...args: Parameters<T>) {
    if (timeout) clearTimeout(timeout)
    timeout = setTimeout(() => func(...args), wait)
  }
}

// 创建防抖的搜索函数
const debouncedSearch = debounce(searchFromDatabase, 300)

// 监听搜索框变化
watch(searchQuery, async (newQuery) => {
  if (newQuery.trim()) {
    // 如果有搜索内容，从数据库搜索
    debouncedSearch()
  } else {
    // 如果搜索框为空，重新加载最近的记录
    await loadRecentHistory()
  }
})

// 添加加载更多记录的函数
const loadMoreHistory = async () => {
  if (!db || isLoadingMore.value || !hasMoreData.value || searchQuery.value.trim()) {
    return
  }
  
  isLoadingMore.value = true
  
  try {
    const isFavoritesTab = selectedTabIndex.value === 1
    
    let sql = `
      SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon 
      FROM clipboard_history
    `
    
    if (isFavoritesTab) {
      sql += ' WHERE is_favorite = 1'
    }
    
    sql += ' ORDER BY timestamp DESC LIMIT ? OFFSET ?'
    
    const rows = await db.select(sql, [50, currentOffset.value])
    
    if (rows.length === 0) {
      hasMoreData.value = false
      console.log('没有更多数据了')
      return
    }
    
    const newItems = rows.map((row: any) => ({
      id: row.id,
      content: row.content,
      type: row.type,
      timestamp: row.timestamp,
      isFavorite: row.is_favorite === 1,
      imagePath: row.image_path ?? null,
      sourceAppName: row.source_app_name ?? 'Unknown',
      sourceAppIcon: row.source_app_icon ?? null
    }))
    
    // 追加新记录到历史列表
    clipboardHistory.value.push(...newItems)
    currentOffset.value += rows.length
    
    console.log(`加载了 ${rows.length} 条更多记录，总计 ${clipboardHistory.value.length} 条`)
    
    // 如果返回的记录数少于请求的数量，说明没有更多数据了
    if (rows.length < 50) {
      hasMoreData.value = false
    }
  } catch (error) {
    console.error('加载更多记录失败:', error)
  } finally {
    isLoadingMore.value = false
  }
}

// 添加滚动处理函数
const handleScroll = (event: Event) => {
  const target = event.target as HTMLElement
  const scrollPosition = target.scrollTop + target.clientHeight
  const scrollHeight = target.scrollHeight
  
  // 当滚动到距离底部100px时，加载更多
  if (scrollHeight - scrollPosition < 100) {
    loadMoreHistory()
  }
}

// 修改加载最近历史记录的函数
const loadRecentHistory = async () => {
  if (!db) return
  
  try {
    const isFavoritesTab = selectedTabIndex.value === 1
    
    let sql = `
      SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon 
      FROM clipboard_history
    `
    
    if (isFavoritesTab) {
      sql += ' WHERE is_favorite = 1'
    }
    
    sql += ' ORDER BY timestamp DESC LIMIT ?'
    
    const rows = await db.select(sql, [MAX_MEMORY_ITEMS])
    
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
    
    // 重置分页状态
    currentOffset.value = clipboardHistory.value.length
    hasMoreData.value = true
    
    console.log(`加载了 ${clipboardHistory.value.length} 条最近的记录`)
  } catch (error) {
    console.error('加载历史记录失败:', error)
  }
}

onMounted(async () => {
  try {
    const dbPath = 'sqlite:clipboard.db'
    console.log('Connecting to database:', dbPath)
    db = await Database.load(dbPath)
    
    // 初始加载最近的历史记录
    await loadRecentHistory()

    // 启动新的剪贴板监听器（使用tauri-plugin-clipboard）
    unlistenClipboard = await startListening()
    console.log('剪贴板监听器已启动（无内存泄漏版本）')

    // 注册剪贴板文本变化监听器
    unlistenClipboardText = await onTextUpdate(async (newText: string) => {
      try {
        console.log('检测到文本剪贴板变化:', newText.length, '字符')
        
        // 限制内容长度
        if (newText && newText.length > 100_000) {
          console.warn('文本内容过长，跳过')
          return
        }
        
        // 检查是否是重复内容
        const duplicateItemId = checkDuplicateContent(newText)
        if (duplicateItemId) {
          console.log('Duplicate text content detected, moving item to front:', duplicateItemId)
          await moveItemToFront(duplicateItemId)
          return
        }

        // 获取当前活动窗口信息
        let sourceAppInfo: { name: string; icon: string | null } = {
          name: 'Unknown',
          icon: null
        }
        
        console.log('🔍 [文本] 开始获取源应用信息...')
        try {
          console.log('🔍 [文本] 调用 get_active_window_info 命令')
          sourceAppInfo = await invoke('get_active_window_info') as { name: string; icon: string | null }
          console.log('✅ [文本] 获取到源应用信息:', {
            name: sourceAppInfo.name,
            hasIcon: sourceAppInfo.icon !== null,
            iconLength: sourceAppInfo.icon ? sourceAppInfo.icon.length : 0
          })
        } catch (error) {
          console.error('❌ [文本] 获取源应用信息失败:', error)
          console.error('❌ [文本] 错误详情:', JSON.stringify(error))
        }

        const item = {
          content: newText,
          type: 'text',
          timestamp: new Date().toISOString(),
          isFavorite: false,
          imagePath: null,
          sourceAppName: sourceAppInfo.name,
          sourceAppIcon: sourceAppInfo.icon
        }
        
        // 插入新记录到数据库
        try {
          await db!.execute(
            `INSERT INTO clipboard_history (content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon) 
             VALUES (?, ?, ?, ?, ?, ?, ?)`,
            [item.content, item.type, item.timestamp, 0, item.imagePath, item.sourceAppName, item.sourceAppIcon]
          )
          const rows = await db!.select(`SELECT last_insert_rowid() as id`)
          const id = rows[0]?.id || Date.now()
          
          // 添加到内存列表的开头
          clipboardHistory.value.unshift(Object.assign({ id }, item))
          
          // 立即执行内存清理
          trimMemoryHistory()
        } catch (dbError) {
          console.error('数据库操作失败:', dbError)
        }
      } catch (error) {
        console.error('Failed to process clipboard text:', error)
      }
    })

    // 注册剪贴板图片变化监听器
    unlistenClipboardImage = await onImageUpdate(async (base64Image: string) => {
      try {
        console.log('检测到图片剪贴板变化:', base64Image.length, '字符')
        
        // 检查图片大小
        if (base64Image && base64Image.length > MAX_IMAGE_PREVIEW_SIZE) {
          console.warn('图片过大，跳过')
          return
        }
        
        // 时间窗口重复检测
        const currentTime = Date.now()
        const timeDiff = currentTime - lastImageProcessTime
        
        if (timeDiff < 2000) { // 2秒内视为重复
          console.log('检测到时间窗口内的重复图片事件，跳过')
          return
        }
        
        lastImageProcessTime = currentTime
        
        // 创建data URL格式
        const imageDataUrl = `data:image/png;base64,${base64Image}`
        
        // 检查是否是重复内容
        const duplicateItemId = checkDuplicateContent(imageDataUrl)
        if (duplicateItemId) {
          console.log('Duplicate image content detected, moving item to front:', duplicateItemId)
          await moveItemToFront(duplicateItemId)
          return
        }

        // 获取当前活动窗口信息
        let sourceAppInfo: { name: string; icon: string | null } = {
          name: 'Unknown',
          icon: null
        }
        
        console.log('🔍 [图片] 开始获取源应用信息...')
        try {
          console.log('🔍 [图片] 调用 get_active_window_info 命令')
          sourceAppInfo = await invoke('get_active_window_info') as { name: string; icon: string | null }
          console.log('✅ [图片] 获取到源应用信息:', {
            name: sourceAppInfo.name,
            hasIcon: sourceAppInfo.icon !== null,
            iconLength: sourceAppInfo.icon ? sourceAppInfo.icon.length : 0
          })
        } catch (error) {
          console.error('❌ [图片] 获取源应用信息失败:', error)
          console.error('❌ [图片] 错误详情:', JSON.stringify(error))
        }

        const item = {
          content: imageDataUrl, // 直接使用base64数据
          type: 'image',
          timestamp: new Date().toISOString(),
          isFavorite: false,
          imagePath: null, // 新插件暂时不支持文件路径
          sourceAppName: sourceAppInfo.name,
          sourceAppIcon: sourceAppInfo.icon
        }
        
        // 插入新记录到数据库
        try {
          await db!.execute(
            `INSERT INTO clipboard_history (content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon) 
             VALUES (?, ?, ?, ?, ?, ?, ?)`,
            [item.content, item.type, item.timestamp, 0, item.imagePath, item.sourceAppName, item.sourceAppIcon]
          )
          const rows = await db!.select(`SELECT last_insert_rowid() as id`)
          const id = rows[0]?.id || Date.now()
          
          // 添加到内存列表的开头
          clipboardHistory.value.unshift(Object.assign({ id }, item))
          
          // 立即执行内存清理
          trimMemoryHistory()
        } catch (dbError) {
          console.error('数据库操作失败:', dbError)
        }
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

    // 设置更频繁的内存清理（每30秒执行一次，更激进的内存管理）
    memoryCleanupInterval = setInterval(() => {
      console.log('执行定期内存清理')
      trimMemoryHistory()
      
      // 清理选中的完整图片内容（如果没有选中图片）
      if (!selectedItem.value || selectedItem.value.type !== 'image') {
        fullImageContent.value = null
      }
      
      // 更积极的历史记录清理
      if (clipboardHistory.value.length > 200) {
        clipboardHistory.value = clipboardHistory.value.slice(0, 150)
        console.log('剪贴板历史记录已清理到150条')
      }
      
      // 清理大文本内容
      clipboardHistory.value.forEach(item => {
        if (item.content && item.content.length > 3000) {
          // 对于长文本，只保留前300字符用于显示
          if (!item.displayContent) {
            item.displayContent = item.content.substring(0, 300) + '...'
          }
        }
      })
      
      // 手动触发垃圾回收（如果可用）
      if (typeof (window as any).gc === 'function') {
        (window as any).gc()
      }
      
      // 清理时间格式化缓存
      if (typeof formatTime === 'function' && formatTime.clearCache) {
        formatTime.clearCache()
      }
    }, 30 * 1000) // 从2分钟减少到30秒
  } catch (error) {
    console.error('Database error:', error)
  }
})

onUnmounted(() => {
  console.log('组件卸载，开始清理资源...')
  
  // 清理键盘事件监听器
  window.removeEventListener('keydown', handleKeyDown)
  
  // 清理Tauri窗口焦点事件监听器
  if (unlistenFocus.value) {
    unlistenFocus.value()
    unlistenFocus.value = null
  }
  
  // 清理剪贴板事件监听器
  if (unlistenClipboardText) {
    unlistenClipboardText()
    unlistenClipboardText = null
  }
  
  if (unlistenClipboardImage) {
    unlistenClipboardImage()
    unlistenClipboardImage = null
  }
  
  if (unlistenClipboard) {
    unlistenClipboard()
    unlistenClipboard = null
  }
  
  // 清理定期内存清理定时器
  if (memoryCleanupInterval) {
    clearInterval(memoryCleanupInterval)
    memoryCleanupInterval = null
  }
  
  // 清理图片内容，释放内存
  fullImageContent.value = null
  
  // 清空剪贴板历史（释放内存）
  clipboardHistory.value.length = 0
  
  // 重置其他状态
  selectedItem.value = null
  searchQuery.value = ''
  
  // 清理数据库连接
  if (db) {
    // 注意：tauri-plugin-sql 的数据库连接通常由插件自动管理
    db = null
  }
  
  // 尝试手动触发垃圾回收
  if (typeof (window as any).gc === 'function') {
    console.log('手动触发垃圾回收')
    ;(window as any).gc()
  }
  
  console.log('资源清理完成')
})

// 添加剪贴板监听器重启功能
const restartClipboardWatcher = async () => {
  try {
    console.log('重启剪贴板监听器...')
    await invoke('start_new_clipboard_watcher')
    console.log('剪贴板监听器重启成功')
  } catch (error) {
    console.error('重启剪贴板监听器失败:', error)
  }
}

// 监听标签页变化
watch(selectedTabIndex, () => {
  // 切换标签页时重置搜索
  searchQuery.value = ''
  // 重置选中项
  selectedItem.value = null
  // 清除完整图片内容
  fullImageContent.value = null
})

// 开发者工具函数
const openDevTools = async () => {
  try {
    const appWindow = getCurrentWindow()
    // 直接调用 openDevtools 方法
    // @ts-ignore
    if (appWindow.openDevtools) {
      // @ts-ignore
      appWindow.openDevtools()
      console.log('Dev tools opened via API')
    } else {
      // 如果方法不存在，尝试使用键盘快捷键
      console.log('openDevtools method not available, trying keyboard shortcut')
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
  } catch (error) {
    console.error('Failed to open dev tools:', error)
    // 尝试使用键盘快捷键作为后备方案
    try {
      document.dispatchEvent(new KeyboardEvent('keydown', {
        key: 'i',
        code: 'KeyI',
        ctrlKey: true,
        shiftKey: true,
        keyCode: 73,
        which: 73,
        bubbles: true
      }))
    } catch (keyError) {
      console.error('Keyboard shortcut also failed:', keyError)
      alert('无法打开开发者工具。请确保在 tauri.conf.json 中设置了 devtools: true')
    }
  }
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

// 增强的内存缓存清理函数
const clearMemoryCache = async () => {
  try {
    // 先调用后端清理
    await invoke('clear_memory_cache')
    console.log('后端内存缓存已清理')
    
    // 前端内存清理
    trimMemoryHistory()
    fullImageContent.value = null
    
    // 清理时间格式化缓存
    if (typeof formatTime === 'function' && formatTime.clearCache) {
      formatTime.clearCache()
    }
    
    // 强制垃圾回收
    if (typeof (window as any).gc === 'function') {
      (window as any).gc()
    }
    
    // 重启剪贴板监听器以清理可能的内存泄漏
    await restartClipboardWatcher()
    
    alert('内存缓存清理完成，剪贴板监听器已重启')
  } catch (error) {
    console.error('清理内存缓存失败:', error)
    alert('清理内存缓存失败: ' + error)
  }
}

// 强制内存清理函数（更激进）
const forceMemoryCleanup = async () => {
  try {
    console.log('开始强制内存清理...')
    
    // 调用后端强制清理
    const result = await invoke('force_memory_cleanup') as string
    console.log('后端强制清理结果:', result)
    
    // 前端激进清理
    clipboardHistory.value = clipboardHistory.value.slice(0, 50) // 只保留50条
    fullImageContent.value = null
    selectedItem.value = null
    searchQuery.value = ''
    
    // 清理所有可能的缓存
    if (typeof formatTime === 'function' && formatTime.clearCache) {
      formatTime.clearCache()
    }
    
    // 多次强制垃圾回收
    for (let i = 0; i < 3; i++) {
      if (typeof (window as any).gc === 'function') {
        (window as any).gc()
      }
      await new Promise(resolve => setTimeout(resolve, 100))
    }
    
    // 重启剪贴板监听器
    await restartClipboardWatcher()
    
    alert(`强制内存清理完成！\n${result}\n历史记录已减少到50条`)
  } catch (error) {
    console.error('强制内存清理失败:', error)
    alert('强制内存清理失败: ' + error)
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
            class="px-3 py-2 text-sm font-medium text-green-600 hover:text-green-900 hover:bg-green-100 rounded-lg transition-colors duration-200"
            @click="restartClipboardWatcher"
            title="重启剪贴板监听器"
          >
            Restart Watcher
          </button>
          <button 
            class="px-3 py-2 text-sm font-medium text-blue-600 hover:text-blue-900 hover:bg-blue-100 rounded-lg transition-colors duration-200"
            @click="clearMemoryCache"
            title="清理内存缓存"
          >
            Clear Cache
          </button>
          <button 
            class="px-3 py-2 text-sm font-medium text-purple-600 hover:text-purple-900 hover:bg-purple-100 rounded-lg transition-colors duration-200"
            @click="forceMemoryCleanup"
            title="强制内存清理（激进模式）"
          >
            Force Clean
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
              <div class="flex-1 overflow-y-auto min-h-0" @scroll="handleScroll">
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
                
                <!-- 加载更多提示 -->
                <div v-if="filteredHistory.length > 0 && !searchQuery" class="py-4 px-3 text-center">
                  <div v-if="isLoadingMore" class="flex items-center justify-center space-x-2">
                    <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-blue-600"></div>
                    <span class="text-xs text-gray-500">Loading more...</span>
                  </div>
                  <div v-else-if="!hasMoreData" class="text-xs text-gray-400">
                    No more items
                  </div>
                  <div v-else class="text-xs text-gray-400">
                    Scroll to load more
                  </div>
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
              <div class="flex-1 overflow-y-auto min-h-0" @scroll="handleScroll">
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
                
                <!-- 加载更多提示 -->
                <div v-if="filteredHistory.length > 0 && !searchQuery" class="py-4 px-3 text-center">
                  <div v-if="isLoadingMore" class="flex items-center justify-center space-x-2">
                    <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-blue-600"></div>
                    <span class="text-xs text-gray-500">Loading more...</span>
                  </div>
                  <div v-else-if="!hasMoreData" class="text-xs text-gray-400">
                    No more items
                  </div>
                  <div v-else class="text-xs text-gray-400">
                    Scroll to load more
                  </div>
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