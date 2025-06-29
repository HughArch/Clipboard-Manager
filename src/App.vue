<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { MagnifyingGlassIcon, StarIcon, Cog6ToothIcon } from '@heroicons/vue/24/outline'
import { StarIcon as StarIconSolid } from '@heroicons/vue/24/solid'
import { Tab, TabList, TabGroup, TabPanels, TabPanel } from '@headlessui/vue'
import Settings from './components/Settings.vue'
import Toast from './components/Toast.vue'
import { useToast } from './composables/useToast'
import { logger } from './composables/useLogger'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'

// 定义类型接口
interface SourceAppInfo {
  name: string
  icon?: string
  bundle_id?: string
}
import Database from '@tauri-apps/plugin-sql'
import { 
  onTextUpdate, 
  onImageUpdate, 
  startListening,
  writeText,
  writeImageBase64
} from 'tauri-plugin-clipboard-api'




// Toast 消息系统
const { toastMessages, removeToast, showSuccess, showError, showWarning, showInfo } = useToast()

// 定义设置类型
interface AppSettings {
  max_history_items: number
  max_history_time: number
  hotkey: string
  auto_start: boolean
}

// 内存中的历史记录限制 - 更严格的限制
const MAX_MEMORY_ITEMS = 300
const MAX_IMAGE_PREVIEW_SIZE = 5 * 1024 * 1024
const MEMORY_CLEAN_INTERVAL = 30* 60 * 1000
const HISTORY_CLEAN_INTERVAL = 60 * 60 * 1000

// 保存设置的函数
const saveSettings = async (settings: AppSettings) => {
  try {
    await invoke('save_settings', { settings })
    logger.info('Settings saved successfully')
  } catch (error) {
    logger.error('Failed to save settings', { error: String(error) })
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

// 处理Toast消息
const handleShowToast = (toast: { type: 'success' | 'error' | 'warning' | 'info', title: string, message?: string, duration?: number }) => {
  switch (toast.type) {
    case 'success':
      showSuccess(toast.title, toast.message, toast.duration)
      break
    case 'error':
      showError(toast.title, toast.message, toast.duration)
      break
    case 'warning':
      showWarning(toast.title, toast.message, toast.duration)
      break
    case 'info':
      showInfo(toast.title, toast.message, toast.duration)
      break
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

// 前一个活动应用程序信息（用于智能粘贴）
const previousActiveApp = ref<SourceAppInfo | null>(null)

// 事件监听器清理函数存储
let unlistenClipboardText: (() => void) | null = null
let unlistenClipboardImage: (() => void) | null = null
let unlistenClipboard: (() => Promise<void>) | null = null
let memoryCleanupInterval: ReturnType<typeof setInterval> | null = null
let historyCleanupInterval: ReturnType<typeof setInterval> | null = null

// 防重复机制：记录最近处理的图片和文本
let lastImageProcessTime = 0
let lastTextContent = '' // 新增：记录最后处理的文本内容
let lastTextProcessTime = 0 // 新增：记录最后处理文本的时间
let isProcessingClipboard = false // 新增：防止并发处理

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
    
    if (removed > 0) {
      logger.debug('内存优化清理完成', { removed, totalItems: clipboardHistory.value.length })
    }
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
      if (firstKey !== undefined) {
      timeCache.delete(firstKey)
      }
    }
    timeCache.set(timestamp, result)
    
    return result
  }
  
  // 添加清理缓存的方法
  ;(formatFunction as any).clearCache = () => {
    timeCache.clear()
    logger.debug('时间格式化缓存已清理')
  }
  
  return formatFunction as typeof formatFunction & { clearCache: () => void }
})()

// 搜索框引用
const searchInputRef = ref<HTMLInputElement | null>(null)
// 存储Tauri事件监听器的unlisten函数
const unlistenFocus = ref<(() => void) | null>(null)
const unlistenPreviousApp = ref<(() => void) | null>(null)

// 清理搜索框并选中第一个条目的函数
const resetToDefault = async () => {
  // 清理搜索框内容
  searchQuery.value = ''
  
  // 如果在搜索模式，退出搜索模式
  if (isInSearchMode) {
    await exitSearchMode()
  }
  
  // 等待下一个tick以确保过滤后的历史列表已更新
  await nextTick()
  
  // 选中第一个条目（如果存在）
  if (filteredHistory.value.length > 0) {
    selectedItem.value = filteredHistory.value[0]
    
    // 滚动到选中的条目
    await scrollToSelectedItem(selectedItem.value.id)
  } else {
    selectedItem.value = null
  }
}

// 自动聚焦搜索框
const focusSearchInput = async () => {
  await nextTick()
  if (searchInputRef.value) {
    searchInputRef.value.focus()
  }
}

// 处理窗口焦点事件，当窗口显示/获得焦点时重置状态
const handleWindowFocus = async () => {
  await resetToDefault()
  await focusSearchInput()
}

// 隐藏应用窗口
const hideWindow = async () => {
  try {
    const appWindow = getCurrentWindow()
    await appWindow.hide()
    logger.debug('窗口已隐藏')
  } catch (error) {
    logger.error('隐藏窗口失败', { error: String(error) })
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
  }
}

const filteredHistory = computed(() => {
  const query = searchQuery.value.toLowerCase()
  
  // 根据标签页筛选：All显示所有，Favorites只显示收藏的
  const items = selectedTabIndex.value === 0 
    ? clipboardHistory.value 
    : clipboardHistory.value.filter(item => item.isFavorite === true)
  
  // 应用搜索过滤
  const result = items.filter(item => 
    item.content?.toLowerCase().includes(query) || false
  )
  
  return result
})

const toggleFavorite = async (item: any) => {
  try {
    const newFavoriteStatus = !item.isFavorite
    
    // 更新数据库
    await db.execute(
      `UPDATE clipboard_history SET is_favorite = ? WHERE id = ?`,
      [newFavoriteStatus ? 1 : 0, item.id]
    )
    
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
      
      // 如果在收藏夹标签页取消收藏
      if (selectedTabIndex.value === 1 && !newFavoriteStatus) {
        // 如果当前选中的是被取消收藏的项，清除选中状态
        if (selectedItem.value?.id === item.id) {
          selectedItem.value = null
        }
      }
    }
  } catch (error) {
    logger.error('切换收藏状态失败', { itemId: item.id, error: String(error) })
  }
}

// 检查是否是重复内容，如果是则返回已有条目的ID
const checkDuplicateContent = async (content: string, contentType: 'text' | 'image'): Promise<number | null> => {
  try {
    // 先检查内存中的历史记录
  const existingItem = clipboardHistory.value.find(item => {
      if (item.type === 'image' && item.imagePath && contentType === 'image') {
      return item.imagePath === content
    }
      return item.content === content && item.type === contentType
    })
    
    if (existingItem) {
      return existingItem.id
    }
    
    // 如果内存中没有找到，检查数据库（防止内存清理导致的漏检）
    if (db) {
      const dbResult = await db.select(
        'SELECT id FROM clipboard_history WHERE content = ? AND type = ? ORDER BY timestamp DESC LIMIT 1',
        [content, contentType]
      )
      if (dbResult.length > 0) {
        return dbResult[0].id
      }
    }
    
    return null
  } catch (error) {
    logger.error('检查重复内容失败', { error: String(error) })
    return null
  }
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
    
    // 在内存中找到该条目
    const itemIndex = clipboardHistory.value.findIndex(item => item.id === itemId)
    if (itemIndex !== -1) {
      // 取出该条目并更新时间戳
      const item = { ...clipboardHistory.value[itemIndex], timestamp: newTimestamp }
      
      // 从原位置移除
      clipboardHistory.value.splice(itemIndex, 1)
      
      // 添加到最前面
      clipboardHistory.value.unshift(item)
      
      // 如果移动的项目就是当前选中的项目，更新选中项目的引用
      if (selectedItem.value?.id === itemId) {
        selectedItem.value = item
      }
      
      // 如果在搜索模式下，也需要更新原始数据中的对应项目
      if (isInSearchMode) {
        const originalIndex = originalClipboardHistory.findIndex(origItem => origItem.id === itemId)
        if (originalIndex !== -1) {
          // 从原位置移除
          originalClipboardHistory.splice(originalIndex, 1)
          // 添加到最前面并更新时间戳
          originalClipboardHistory.unshift({ ...item, timestamp: newTimestamp })
        }
      }
    } else {
      // 如果内存中没有找到，从数据库重新加载该条目
      const dbResult = await db.select(
        'SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon FROM clipboard_history WHERE id = ?',
        [itemId]
      )
      
      if (dbResult.length > 0) {
        const row = dbResult[0]
        const item = {
          id: row.id,
          content: row.content,
          type: row.type,
          timestamp: newTimestamp, // 使用新的时间戳
          isFavorite: row.is_favorite === 1,
          imagePath: row.image_path ?? null,
          sourceAppName: row.source_app_name ?? 'Unknown',
          sourceAppIcon: row.source_app_icon ?? null
        }
        
        // 添加到内存列表的开头
        clipboardHistory.value.unshift(item)
        
        // 执行内存清理以防止列表过长
        trimMemoryHistory()
      }
    }
  } catch (error) {
    logger.error('移动项目到前面失败', { itemId, error: String(error) })
  }
}

// 复制内容到系统剪贴板并智能粘贴到目标应用
const copyToClipboard = async (item: any) => {
  if (!item) return
  
  try {
    logger.debug('开始智能复制和粘贴', { type: item.type, id: item.id })
    
    // 使用之前保存的目标应用信息（在快捷键触发时获取的）
    let targetApp: SourceAppInfo | null = previousActiveApp.value
    
    // 如果没有预保存的信息，则尝试获取（但此时可能已经不准确）
    if (!targetApp) {
      try {
        targetApp = await invoke('get_active_window_info') as SourceAppInfo
      } catch (error) {
        logger.warn('获取活动窗口信息失败', { error: String(error) })
        targetApp = null
      }
    }
    
    // 准备要复制的内容
    let contentToCopy = item.content
    
    // 对于图片，如果是当前选中的项目且有完整图片内容，则使用完整内容
    if (item.type === 'image' && selectedItem.value?.id === item.id && fullImageContent.value) {
      contentToCopy = fullImageContent.value
    } else if (item.type === 'image' && item.imagePath) {
      // 如果是旧格式的图片（有 imagePath），尝试加载完整图片
      try {
        const fullImage = await invoke('load_image_file', { imagePath: item.imagePath }) as string
        contentToCopy = fullImage
      } catch (error) {
        logger.warn('加载完整图片失败，使用缩略图', { error: String(error) })
        contentToCopy = item.content
      }
    }
    
    // 获取窗口引用，准备并行操作
    const appWindow = getCurrentWindow()
    
    // 并行执行剪贴板写入和窗口隐藏操作
    const [, ] = await Promise.all([
      // 写入系统剪贴板
      (async () => {
        if (item.type === 'text') {
          await writeText(contentToCopy)
        } else if (item.type === 'image') {
          // 提取 base64 数据（去掉 data:image/png;base64, 前缀）
          const base64Data = contentToCopy?.replace(/^data:image\/[^;]+;base64,/, '') || ''
          if (base64Data) {
            await writeImageBase64(base64Data)
          } else {
            throw new Error('Invalid image data')
          }
        }
      })(),
      // 隐藏窗口
      appWindow.hide()
    ])
    
    // 极短等待，让窗口隐藏生效
    await new Promise(resolve => setTimeout(resolve, 50))
    
    // 使用智能粘贴：如果有目标应用信息，就激活目标应用再粘贴
    if (targetApp && targetApp.name && targetApp.name !== 'Unknown' && 
        !targetApp.name.includes('Clipboard') && !targetApp.name.includes('clipboard')) {
      logger.debug('执行智能粘贴', { targetApp: targetApp.name })
      await invoke('smart_paste_to_app', { 
        appName: targetApp.name,
        bundleId: targetApp.bundle_id || null
      })
    } else {
      logger.debug('执行普通粘贴')
      await invoke('auto_paste')
    }
    
  } catch (error) {
    logger.error('复制和粘贴失败', { error: String(error) })
    // 如果出错，重新显示窗口
    try {
      const appWindow = getCurrentWindow()
      await appWindow.show()
    } catch (showError) {
      logger.error('显示窗口失败', { error: String(showError) })
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

  // 确保filteredHistory是最新的，避免状态不同步
  const currentFilteredList = filteredHistory.value
  const currentIndex = currentFilteredList.findIndex((item: any) => item.id === selectedItem.value?.id)
  let newIndex = currentIndex

  if (e.key === 'ArrowUp') {
    e.preventDefault()
    if (currentIndex === -1) {
      // 如果当前没有选中项，选中第一个
      newIndex = 0
    } else {
      newIndex = currentIndex > 0 ? currentIndex - 1 : currentFilteredList.length - 1
    }
  } else if (e.key === 'ArrowDown') {
    e.preventDefault()
    if (currentIndex === -1) {
      // 如果当前没有选中项，选中第一个
      newIndex = 0
    } else {
      newIndex = currentIndex < currentFilteredList.length - 1 ? currentIndex + 1 : 0
    }
  } else if (e.key === 'Enter') {
    e.preventDefault()
    // 按Enter键复制当前选中的项目到剪贴板
    if (selectedItem.value) {
      copyToClipboard(selectedItem.value)
    }
    return
  }

  // 确保新索引有效
  if (newIndex >= 0 && newIndex < currentFilteredList.length && newIndex !== currentIndex) {
    const newSelectedItem = currentFilteredList[newIndex]
    
    // 验证新选中的项目确实存在且有有效ID
    if (newSelectedItem && newSelectedItem.id) {
      selectedItem.value = newSelectedItem
      
      // 滚动到新选中的条目
      nextTick(() => {
        scrollToSelectedItem(newSelectedItem.id)
      })
    }
  }
}

// 处理双击事件
const handleDoubleClick = (item: any) => {
  copyToClipboard(item)
}

const handleTabChange = async (index: number) => {
  selectedTabIndex.value = index
  // 重置搜索和选中状态
  searchQuery.value = ''
  selectedItem.value = null
  
  // 重置分页状态
  currentOffset.value = 0
  hasMoreData.value = true
  
  // 如果在搜索模式，先退出搜索模式
  if (isInSearchMode) {
    await exitSearchMode()
  } else {
    // 重新加载对应标签页的数据
    await loadRecentHistory()
  }
  
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
        const fullImage = await invoke('load_image_file', { imagePath: newItem.imagePath }) as string
        
        // 检查图片大小，如果过大则不在内存中保存
        if (fullImage.length > MAX_IMAGE_PREVIEW_SIZE) {
          fullImageContent.value = newItem.content
        } else {
          fullImageContent.value = fullImage
        }
      } else {
        // 新插件模式：直接使用content中的base64数据
        if (newItem.content && typeof newItem.content === 'string') {
          // 检查图片大小
          if (newItem.content.length > MAX_IMAGE_PREVIEW_SIZE) {
            // 即使过大也显示，因为这是唯一的数据源
            fullImageContent.value = newItem.content
          } else {
            fullImageContent.value = newItem.content
          }
        } else {
          fullImageContent.value = null
        }
      }
    } catch (error) {
      logger.warn('加载图片失败', { error: String(error) })
      // 如果加载失败，尝试使用content作为后备
      fullImageContent.value = (newItem.content && typeof newItem.content === 'string') ? newItem.content : null
    }
  } else {
    fullImageContent.value = null
  }
})

// 保存原始数据的变量
let originalClipboardHistory: any[] = []
let isInSearchMode = false

// 添加数据库搜索函数
const searchFromDatabase = async () => {
  if (!db || !searchQuery.value.trim()) {
    return
  }
  
  isSearching.value = true
  
  try {
    // 如果是第一次搜索，保存当前的内存数据
    if (!isInSearchMode) {
      originalClipboardHistory = [...clipboardHistory.value]
      isInSearchMode = true
      logger.debug('进入搜索模式', { originalDataCount: originalClipboardHistory.length })
    }
    
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
    
    // 将搜索结果转换为前端格式，确保去重
    const seenIds = new Set()
    const searchResults = rows
      .map((row: any) => ({
        id: row.id,
        content: row.content,
        type: row.type,
        timestamp: row.timestamp,
        isFavorite: row.is_favorite === 1,
        imagePath: row.image_path ?? null,
        sourceAppName: row.source_app_name ?? 'Unknown',
        sourceAppIcon: row.source_app_icon ?? null
      }))
      .filter((item: any) => {
        if (seenIds.has(item.id)) {
          return false
        }
        seenIds.add(item.id)
        return true
      })
    
    // 更新内存中的历史记录为搜索结果
    clipboardHistory.value = searchResults
    
    // 重置选中状态，避免状态混乱
    selectedItem.value = null
    
    logger.debug('数据库搜索完成', { resultCount: searchResults.length })
  } catch (error) {
    logger.error('数据库搜索失败', { error: String(error) })
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

// 退出搜索模式，恢复原始数据
const exitSearchMode = async () => {
  if (isInSearchMode) {
    logger.debug('退出搜索模式，恢复原始数据', { originalDataCount: originalClipboardHistory.length })
    
    // 合并在搜索期间可能新增的数据
    const currentNewestItems = clipboardHistory.value.filter((item: any) => {
      // 检查是否是在搜索期间新增的（时间戳比保存的最新项目更新）
      if (originalClipboardHistory.length === 0) return true
      
      const newestOriginalTimestamp = new Date(originalClipboardHistory[0]?.timestamp || 0).getTime()
      const itemTimestamp = new Date(item.timestamp).getTime()
      
      return itemTimestamp > newestOriginalTimestamp
    })
    
    // 去重：从原始数据中移除可能重复的项目
    const deduplicatedOriginal = originalClipboardHistory.filter((originalItem: any) => {
      return !currentNewestItems.some((newItem: any) => newItem.id === originalItem.id)
    })
    
    // 使用Set进行最终去重，确保没有重复ID
    const allItems = [...currentNewestItems, ...deduplicatedOriginal]
    const seenIds = new Set()
    const finalDeduplicatedItems = allItems.filter((item: any) => {
      if (seenIds.has(item.id)) {
        return false
      }
      seenIds.add(item.id)
      return true
    })
    
    // 合并数据：确保按时间戳排序
    clipboardHistory.value = finalDeduplicatedItems.sort((a: any, b: any) => 
      new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime()
    )
    
    logger.debug('数据恢复完成', { newItemsCount: currentNewestItems.length, originalItemsCount: deduplicatedOriginal.length, totalItems: finalDeduplicatedItems.length })
    
    // 清空保存的数据和重置选中状态
    originalClipboardHistory = []
    isInSearchMode = false
    selectedItem.value = null
  } else {
    // 如果不在搜索模式，正常重新加载
    await loadRecentHistory()
  }
}

// 监听搜索框变化
watch(searchQuery, async (newQuery) => {
  if (newQuery.trim()) {
    // 如果有搜索内容，从数据库搜索
    debouncedSearch()
  } else {
    // 如果搜索框为空，退出搜索模式
    await exitSearchMode()
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
      logger.debug('没有更多数据了')
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
    
    logger.debug('加载了更多记录', { loadedCount: rows.length, totalCount: clipboardHistory.value.length })
    
    // 如果返回的记录数少于请求的数量，说明没有更多数据了
    if (rows.length < 50) {
      hasMoreData.value = false
    }
  } catch (error) {
    logger.error('加载更多记录失败', { error: String(error) })
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
    
    // 确保去重
    const seenIds = new Set()
    const deduplicatedHistory = rows
      .map((row: any) => ({
        id: row.id,
        content: row.content,
        type: row.type,
        timestamp: row.timestamp,
        isFavorite: row.is_favorite === 1,
        imagePath: row.image_path ?? null,
        sourceAppName: row.source_app_name ?? 'Unknown',
        sourceAppIcon: row.source_app_icon ?? null
      }))
      .filter((item: any) => {
        if (seenIds.has(item.id)) {
          return false
        }
        seenIds.add(item.id)
        return true
      })
    
    clipboardHistory.value = deduplicatedHistory
    
    // 重置分页状态和选中状态
    currentOffset.value = clipboardHistory.value.length
    hasMoreData.value = true
    selectedItem.value = null
    
    logger.debug('加载了最近的记录', { totalCount: clipboardHistory.value.length })
  } catch (error) {
    logger.error('加载历史记录失败', { error: String(error) })
  }
}

onMounted(async () => {
  try {
    // 初始化日志系统
    logger.info('应用程序启动', { timestamp: new Date().toISOString() })
    
    const dbPath = 'sqlite:clipboard.db'
    logger.info('连接数据库', { dbPath })
    db = await Database.load(dbPath)
    
    // 初始加载最近的历史记录
    await loadRecentHistory()

    // 启动新的剪贴板监听器（使用tauri-plugin-clipboard）
    unlistenClipboard = await startListening()
    logger.info('剪贴板监听器已启动（无内存泄漏版本）')

    // 注册剪贴板文本变化监听器
    unlistenClipboardText = await onTextUpdate(async (newText: string) => {
      try {
        logger.debug('检测到文本剪贴板变化', { length: newText.length })
        
        // 防止并发处理
        if (isProcessingClipboard) {
          logger.debug('正在处理其他剪贴板事件，跳过')
          return
        }
        
        // 限制内容长度
        if (newText && newText.length > 100_000) {
          logger.warn('文本内容过长，跳过')
          return
        }
        
        // 时间窗口重复检测（防止快速重复复制）
        const currentTime = Date.now()
        const timeDiff = currentTime - lastTextProcessTime
        
        if (timeDiff < 1000 && lastTextContent === newText) { // 1秒内相同内容视为重复
          logger.debug('检测到时间窗口内的重复文本事件，跳过')
          return
        }
        
        // 设置处理标志
        isProcessingClipboard = true
        lastTextContent = newText
        lastTextProcessTime = currentTime
        
        try {
        // 检查是否是重复内容
          const duplicateItemId = await checkDuplicateContent(newText, 'text')
        if (duplicateItemId) {
                      logger.debug('Duplicate text content detected, moving item to front', { itemId: duplicateItemId })
          await moveItemToFront(duplicateItemId)
          return
          }
        } finally {
          // 在 finally 块外处理后续逻辑，但先清除标志
          // 标志将在函数末尾清除
        }

        // 获取当前活动窗口信息
        let sourceAppInfo: SourceAppInfo = {
          name: 'Unknown',
          icon: undefined,
          bundle_id: undefined
        }
        
        try {
          const appInfo = await invoke('get_active_window_info_for_clipboard') as SourceAppInfo
          sourceAppInfo = appInfo
        } catch (error) {
          logger.error('获取源应用信息失败', { error: String(error) })
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
          
          const newItem = Object.assign({ id }, item)
          
          // 检查内存中是否已存在相同ID的项目，避免重复
          const existingIndex = clipboardHistory.value.findIndex((historyItem: any) => historyItem.id === id)
          if (existingIndex === -1) {
            // 添加到内存列表的开头
            clipboardHistory.value.unshift(newItem)
            
            // 如果在搜索模式下，也需要添加到原始数据
            if (isInSearchMode) {
              const originalExistingIndex = originalClipboardHistory.findIndex((origItem: any) => origItem.id === id)
              if (originalExistingIndex === -1) {
                originalClipboardHistory.unshift(newItem)
              }
            }
          }
          
          // 立即执行内存清理
          trimMemoryHistory()
        } catch (dbError) {
          logger.error('数据库操作失败', { error: String(dbError) })
        }
      } catch (error) {
        logger.error('处理剪贴板文本失败', { error: String(error) })
      } finally {
        // 确保在所有情况下都清除处理标志
        isProcessingClipboard = false
      }
    })

    // 注册剪贴板图片变化监听器
    unlistenClipboardImage = await onImageUpdate(async (base64Image: string) => {
      try {
        // 防止并发处理
        if (isProcessingClipboard) {
          logger.debug('正在处理其他剪贴板事件，跳过')
          return
        }
        
        // 检查图片大小
        if (base64Image && base64Image.length > MAX_IMAGE_PREVIEW_SIZE) {
          logger.warn('图片过大，跳过')
          return
        }
        
        // 时间窗口重复检测
        const currentTime = Date.now()
        const timeDiff = currentTime - lastImageProcessTime
        
        if (timeDiff < 2000) { // 2秒内视为重复
          logger.debug('检测到时间窗口内的重复图片事件，跳过')
          return
        }
        
        // 设置处理标志
        isProcessingClipboard = true
        lastImageProcessTime = currentTime
        
        // 创建data URL格式
        const imageDataUrl = `data:image/png;base64,${base64Image}`
        
        // 检查是否是重复内容
        const duplicateItemId = await checkDuplicateContent(imageDataUrl, 'image')
        if (duplicateItemId) {
          logger.debug('Duplicate image content detected, moving item to front', { itemId: duplicateItemId })
          await moveItemToFront(duplicateItemId)
          return
        }

        // 获取当前活动窗口信息
        let sourceAppInfo: SourceAppInfo = {
          name: 'Unknown',
          icon: undefined,
          bundle_id: undefined
        }
        
        try {
          const appInfo = await invoke('get_active_window_info_for_clipboard') as SourceAppInfo
          sourceAppInfo = appInfo
        } catch (error) {
          logger.error('获取源应用信息失败', { error: String(error) })
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
          
          const newItem = Object.assign({ id }, item)
          
          // 检查内存中是否已存在相同ID的项目，避免重复
          const existingIndex = clipboardHistory.value.findIndex((historyItem: any) => historyItem.id === id)
          if (existingIndex === -1) {
            // 添加到内存列表的开头
            clipboardHistory.value.unshift(newItem)
            
            // 如果在搜索模式下，也需要添加到原始数据
            if (isInSearchMode) {
              const originalExistingIndex = originalClipboardHistory.findIndex((origItem: any) => origItem.id === id)
              if (originalExistingIndex === -1) {
                originalClipboardHistory.unshift(newItem)
              }
            }
          }
          
          // 立即执行内存清理
          trimMemoryHistory()
        } catch (dbError) {
          logger.error('数据库操作失败', { error: String(dbError) })
        }
      } catch (error) {
        logger.error('处理剪贴板图片失败', { error: String(error) })
      } finally {
        // 确保在所有情况下都清除处理标志
        isProcessingClipboard = false
      }
    })

    window.addEventListener('keydown', handleKeyDown)
    
    // 处理窗口关闭事件，隐藏到托盘而不是关闭
    const appWindow = getCurrentWindow()
    
    // 监听前一个活动应用程序信息事件
    const unlistenPreviousAppFunc = await appWindow.listen<SourceAppInfo>('previous-app-info', (event) => {
      previousActiveApp.value = event.payload
    })
    
    // 将unlisten函数存储到ref中
    unlistenPreviousApp.value = unlistenPreviousAppFunc
    
    // 监听窗口焦点事件
    const unlistenFocusFunc = await appWindow.onFocusChanged(({ payload: focused }) => {
      if (focused) {
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
      logger.debug('窗口隐藏到系统托盘')
    })
    
    // 组件挂载后自动聚焦搜索框
    await focusSearchInput()
    
    // 开发环境下将调试函数绑定到window对象
    if (process.env.NODE_ENV === 'development') {
      (window as any).checkDataConsistency = checkDataConsistency
    }
    
    // 定期内存清理
    memoryCleanupInterval = setInterval(() => {
      trimMemoryHistory()
      
      // 清理选中的完整图片内容（如果没有选中图片）
      if (!selectedItem.value || selectedItem.value.type !== 'image') {
        fullImageContent.value = null
      }
      
      // 手动触发垃圾回收（如果可用）
      if (typeof (window as any).gc === 'function') {
        (window as any).gc()
      }
      
      // 清理时间格式化缓存
      if (typeof formatTime === 'function' && formatTime.clearCache) {
        formatTime.clearCache()
      }
    }, MEMORY_CLEAN_INTERVAL) // 从2分钟减少到30秒

    // 设置定期数据库历史清理
    // 这将清理超过设置时间限制的过期历史记录，释放存储空间
    historyCleanupInterval = setInterval(async () => {
      try {
        await invoke('cleanup_history')
        logger.info('定期数据库历史清理完成')
        
        // 清理完成后，如果不在搜索模式，重新加载最近的记录以反映清理后的状态
        if (!isInSearchMode && !searchQuery.value.trim()) {
          await loadRecentHistory()
        }
      } catch (error) {
        logger.error('定期数据库历史清理失败', { error: String(error) })
      }
    }, HISTORY_CLEAN_INTERVAL) // 每小时执行一次 (60分钟 * 60秒 * 1000毫秒)
    
  } catch (error) {
    logger.error('数据库错误', { error: String(error) })
  }
})

onUnmounted(() => {
  logger.debug('组件卸载，开始清理资源...')
  
  // 清理键盘事件监听器
  window.removeEventListener('keydown', handleKeyDown)
  
  // 清理Tauri窗口焦点事件监听器
  if (unlistenFocus.value) {
    unlistenFocus.value()
    unlistenFocus.value = null
  }
  
  // 清理前一个活动应用信息事件监听器
  if (unlistenPreviousApp.value) {
    unlistenPreviousApp.value()
    unlistenPreviousApp.value = null
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
  
  // 清理定期历史清理定时器
  if (historyCleanupInterval) {
    clearInterval(historyCleanupInterval)
    historyCleanupInterval = null
  }
  
  // 清理图片内容，释放内存
  fullImageContent.value = null
  
  // 清空剪贴板历史（释放内存）
  clipboardHistory.value.length = 0
  
  // 重置其他状态
  selectedItem.value = null
  searchQuery.value = ''
  
  // 清理搜索模式状态
  isInSearchMode = false
  originalClipboardHistory = []
  
  // 清理数据库连接
  if (db) {
    // 注意：tauri-plugin-sql 的数据库连接通常由插件自动管理
    db = null
  }
  
  // 尝试手动触发垃圾回收
  if (typeof (window as any).gc === 'function') {
    ;(window as any).gc()
  }
  
  logger.debug('资源清理完成')
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



// 数据一致性检查函数（调试用）
const checkDataConsistency = () => {
  const report = {
    clipboardHistoryLength: clipboardHistory.value.length,
    filteredHistoryLength: filteredHistory.value.length,
    selectedItemId: selectedItem.value?.id,
    isInSearchMode,
    originalHistoryLength: originalClipboardHistory.length
  }
  
  logger.debug('数据一致性检查', report)
  
  // 检查重复ID
  const ids = clipboardHistory.value.map((item: any) => item.id)
  const uniqueIds = new Set(ids)
  if (ids.length !== uniqueIds.size) {
    // 找出重复的ID
    const duplicates: any[] = []
    const seen = new Set()
    ids.forEach((id: any) => {
      if (seen.has(id)) {
        duplicates.push(id)
      }
      seen.add(id)
    })
    logger.warn('发现重复ID', { duplicateIds: duplicates })
  } else {
    logger.debug('数据一致性检查通过：无重复ID')
  }
  
  // 检查选中项是否在列表中
  if (selectedItem.value) {
    const found = filteredHistory.value.find((item: any) => item.id === selectedItem.value?.id)
    if (!found) {
      logger.warn('选中项不在过滤列表中', { selectedItemId: selectedItem.value.id })
    } else {
      logger.debug('选中项有效')
    }
  }
}

// 重置数据库函数（仅用于开发环境修复迁移冲突）
const resetDatabase = async () => {
  if (confirm('确定要重置数据库吗？这将删除所有剪贴板历史记录！')) {
    try {
      await invoke('reset_database')
      logger.info('数据库重置成功')
      alert('数据库重置成功！请重启应用程序。')
      // 重新加载页面以重新初始化
      window.location.reload()
    } catch (error) {
      logger.error('重置数据库失败', { error: String(error) })
      alert('重置数据库失败: ' + error)
    }
  }
}




</script>

<template>
  <div class="h-screen flex flex-col bg-gray-50 rounded-lg shadow-2xl border border-gray-200 overflow-hidden">
    <!-- Custom Title Bar -->
    <div class="bg-white border-b border-gray-200 flex items-center justify-between h-8 select-none" data-tauri-drag-region>
      <div class="flex items-center px-3" data-tauri-drag-region>
        <span class="text-xs text-gray-600">Clipboard Manager</span>
      </div>
      <div class="flex">
        <button 
          @click="hideWindow"
          class="w-10 h-8 hover:bg-red-100 hover:text-red-600 flex items-center justify-center transition-colors"
          title="Hide Window"
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
          <!-- <button 
            class="px-3 py-2 text-sm font-medium text-gray-600 hover:text-gray-900 hover:bg-gray-100 rounded-lg transition-colors duration-200"
            @click="openDevTools"
          >
            Dev Tools
          </button> -->
          <!-- <button 
            class="px-3 py-2 text-sm font-medium text-green-600 hover:text-green-900 hover:bg-green-100 rounded-lg transition-colors duration-200"
            @click="restartClipboardWatcher"
            title="重启剪贴板监听器"
          >
            Restart Watcher
          </button> -->
          <!-- <button 
            class="px-3 py-2 text-sm font-medium text-blue-600 hover:text-blue-900 hover:bg-blue-100 rounded-lg transition-colors duration-200"
            @click="clearMemoryCache"
            title="清理内存缓存"
          >
            Clear Cache
          </button> -->
          <!-- <button 
            class="px-3 py-2 text-sm font-medium text-purple-600 hover:text-purple-900 hover:bg-purple-100 rounded-lg transition-colors duration-200"
            @click="forceMemoryCleanup"
            title="强制内存清理（激进模式）"
          >
            Force Clean
          </button> -->
          <!-- <button 
            class="px-3 py-2 text-sm font-medium text-green-600 hover:text-green-900 hover:bg-green-100 rounded-lg transition-colors duration-200"
            @click="manualCleanupHistory"
            title="清理过期历史记录"
          >
            Cleanup
          </button> -->
          <button 
            class="px-3 py-2 text-sm font-medium text-red-600 hover:text-red-900 hover:bg-red-100 rounded-lg transition-colors duration-200"
            @click="resetDatabase"
            title="重置数据库"
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
          <div class="flex-shrink-0 bg-gradient-to-r from-gray-50 to-gray-100 px-3 py-1 border-b border-gray-200">
            <TabList class="flex items-center justify-center space-x-0.5 bg-gray-100 p-0.5 rounded-lg shadow-inner max-w-[200px] mx-auto">
              <!-- All 标签页 -->
              <Tab v-slot="{ selected }" as="template">
                <button
                  class="relative px-4 py-1.5 text-sm font-medium rounded-md transition-all duration-200 ease-out focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1 focus:ring-offset-gray-100 min-w-[80px]"
                  :class="[
                    selected
                      ? 'text-white bg-gradient-to-r from-blue-500 to-blue-600 shadow-md shadow-blue-500/20'
                      : 'text-gray-600 hover:text-gray-800 hover:bg-white/70 hover:shadow-sm'
                  ]"
                >
                  <span class="flex items-center justify-center space-x-1.5">
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
                  class="relative px-4 py-1.5 text-sm font-medium rounded-md transition-all duration-200 ease-out focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-1 focus:ring-offset-gray-100 min-w-[80px]"
                  :class="[
                    selected
                      ? 'text-white bg-gradient-to-r from-amber-500 to-yellow-500 shadow-md shadow-amber-500/20'
                      : 'text-gray-600 hover:text-gray-800 hover:bg-white/70 hover:shadow-sm'
                  ]"
                >
                  <span class="flex items-center justify-center space-x-1.5">
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
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id && selectedItem?.id !== undefined,
                    'hover:bg-gray-50': selectedItem?.id !== item.id || selectedItem?.id === undefined
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
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id && selectedItem?.id !== undefined,
                    'hover:bg-gray-50': selectedItem?.id !== item.id || selectedItem?.id === undefined
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
    <Settings 
      v-model:show="showSettings" 
      @save-settings="handleSaveSettings"
      @show-toast="handleShowToast"
    />
    
    <!-- Toast notifications -->
    <Toast 
      :messages="toastMessages" 
      @remove="removeToast" 
    />
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
  filter: brightness(1.05);
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

/* 现代化标签页样式 */
.modern-tab-container {
  background: linear-gradient(135deg, #f8fafc 0%, #f1f5f9 100%);
  border-bottom: 1px solid #e2e8f0;
  box-shadow: inset 0 -1px 0 0 rgba(255, 255, 255, 0.1);
}

.modern-tab-list {
  background: rgba(226, 232, 240, 0.6);
  backdrop-filter: blur(10px);
  border: 1px solid rgba(226, 232, 240, 0.8);
  box-shadow: 
    0 4px 6px -1px rgba(0, 0, 0, 0.1),
    0 2px 4px -1px rgba(0, 0, 0, 0.06),
    inset 0 1px 0 0 rgba(255, 255, 255, 0.1);
}

.modern-tab-button {
  position: relative;
  overflow: hidden;
}

.modern-tab-button::before {
  content: '';
  position: absolute;
  top: 0;
  left: -100%;
  width: 100%;
  height: 100%;
  background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.4), transparent);
  transition: left 0.6s;
}

.modern-tab-button:hover::before {
  left: 100%;
}

/* 选中状态的简洁效果 */
.tab-selected-all {
  background: linear-gradient(135deg, #3b82f6 0%, #1d4ed8 100%);
  box-shadow: 
    0 4px 6px -1px rgba(59, 130, 246, 0.3),
    0 2px 4px -1px rgba(59, 130, 246, 0.2),
    inset 0 1px 0 0 rgba(255, 255, 255, 0.1);
}

.tab-selected-favorites {
  background: linear-gradient(135deg, #f59e0b 0%, #d97706 100%);
  box-shadow: 
    0 4px 6px -1px rgba(245, 158, 11, 0.3),
    0 2px 4px -1px rgba(245, 158, 11, 0.2),
    inset 0 1px 0 0 rgba(255, 255, 255, 0.1);
}

/* 按钮文字和图标样式 */
.tab-content {
  position: relative;
  z-index: 10;
  font-weight: 600;
  letter-spacing: 0.025em;
}

/* 悬停状态简洁 */
.modern-tab-button:not(.tab-selected-all):not(.tab-selected-favorites):hover {
  background: rgba(255, 255, 255, 0.8);
  box-shadow: 
    0 2px 4px -1px rgba(0, 0, 0, 0.1),
    0 1px 2px -1px rgba(0, 0, 0, 0.06);
}

/* 焦点状态 */
.modern-tab-button:focus {
  outline: none;
  ring: 2px;
  ring-color: #3b82f6;
  ring-offset: 2px;
}



/* 响应式优化 */
@media (max-width: 640px) {
  .modern-tab-list {
    max-width: 180px;
  }
  
  .modern-tab-button {
    min-width: 70px;
    padding: 0.375rem 0.75rem;
    font-size: 0.875rem;
  }
}

/* 紧凑型标签页样式 */
.compact-tab-container {
  background: linear-gradient(135deg, #f8fafc 0%, #f1f5f9 100%);
  padding: 0.5rem 0.75rem;
}

.compact-tab-list {
  background: rgba(226, 232, 240, 0.7);
  backdrop-filter: blur(8px);
  border: 1px solid rgba(226, 232, 240, 0.6);
  border-radius: 0.5rem;
  padding: 0.125rem;
  box-shadow: 
    0 2px 4px -1px rgba(0, 0, 0, 0.06),
    inset 0 1px 0 0 rgba(255, 255, 255, 0.1);
}

.compact-tab-button {
  padding: 0.375rem 1rem;
  min-width: 80px;
  font-weight: 500;
  border-radius: 0.375rem;
}

/* 选中状态的简洁效果 */
.compact-selected {
  box-shadow: 
    0 2px 4px -1px rgba(59, 130, 246, 0.25),
    0 1px 2px -1px rgba(59, 130, 246, 0.15);
}

/* 图标和文字间距优化 */
.compact-tab-content {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.375rem;
}

.compact-tab-icon {
  width: 0.875rem;
  height: 0.875rem;
  flex-shrink: 0;
}

/* 高分辨率屏幕优化 */
@media (-webkit-min-device-pixel-ratio: 2), (min-resolution: 2dppx) {
  .modern-tab-list {
    border-width: 0.5px;
  }
}

/* 暗色模式适配预留 */
@media (prefers-color-scheme: dark) {
  .modern-tab-container {
    background: linear-gradient(135deg, #1e293b 0%, #0f172a 100%);
    border-bottom-color: #334155;
  }
  
  .modern-tab-list {
    background: rgba(51, 65, 85, 0.6);
    border-color: rgba(51, 65, 85, 0.8);
  }
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