<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick, shallowRef, triggerRef } from 'vue'
import { MagnifyingGlassIcon, StarIcon, Cog6ToothIcon, TrashIcon } from '@heroicons/vue/24/outline'
import { StarIcon as StarIconSolid } from '@heroicons/vue/24/solid'
import Settings from './components/Settings.vue'
import Toast from './components/Toast.vue'
import ConfirmDialog from './components/ConfirmDialog.vue'
import { useToast } from './composables/useToast'
import { logger } from './composables/useLogger'
// import { useImageCache } from './composables/useImageCache' // 暂时注释掉未使用的导入
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

// 图片缓存系统
// const imageCache = useImageCache() // 暂时注释掉未使用的变量

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

// Mock data - 使用 shallowRef 优化大量数据的性能
const clipboardHistory = shallowRef<any[]>([])
const searchQuery = ref('')
const selectedItem = ref(clipboardHistory.value[0])
const showSettings = ref(false)
const selectedTabIndex = ref(0)
const selectedGroupId = ref<number | null>(null) // 当前选中的分组ID
const showGroupDropdown = ref(false) // 是否显示分组下拉菜单
const searchPlaceholders = ['搜索剪贴板历史...', '搜索文本...', '搜索图片...', '搜索收藏...', '搜索分组...']

// 计算有条目的分组
const availableGroups = computed(() => groups.value.filter(g => g.item_count > 0))

// 当前标签页信息
const currentTabInfo = computed(() => {
  const tabNames = ['全部', '文本', '图片', '收藏', '分组']
  const tabIndex = selectedTabIndex.value
  
  let displayName = tabNames[tabIndex] || '未知'
  let emptyMessage = '暂无条目'
  
  if (tabIndex === 4 && selectedGroupId.value) {
    const group = groups.value.find(g => g.id === selectedGroupId.value)
    displayName = group ? `分组: ${group.name}` : '未知分组'
    emptyMessage = '该分组暂无条目'
  } else if (tabIndex === 1) {
    emptyMessage = '暂无文本条目'
  } else if (tabIndex === 2) {
    emptyMessage = '暂无图片条目'  
  } else if (tabIndex === 3) {
    emptyMessage = '暂无收藏条目'
  }
  
  return {
    name: displayName,
    emptyMessage,
    isGroupMode: tabIndex === 4,
    showGroupHeader: tabIndex === 4 && selectedGroupId.value
  }
})
const fullImageContent = ref<string | null>(null) // 存储完整图片的 base64 数据
const thumbnailCache = shallowRef(new Map<string, string>()) // 缩略图缓存 - 使用 shallowRef 优化性能
let db: Awaited<ReturnType<any>> | null = null
const isSearching = ref(false) // 添加搜索状态标识
const isLoadingMore = ref(false) // 添加加载更多状态
const hasMoreData = ref(true) // 是否还有更多数据
const currentOffset = ref(0) // 当前加载的偏移量
const allDataLoaded = ref(false) // 是否已加载全部数据到内存

// 备注管理相关状态
const showNoteDialog = ref(false) // 是否显示备注编辑对话框
const editingNoteItem = ref<any>(null) // 正在编辑备注的条目
const noteText = ref('') // 备注文本

// 右键菜单相关状态
const showContextMenu = ref(false) // 是否显示右键菜单
const contextMenuPosition = ref({ x: 0, y: 0 }) // 右键菜单位置
const contextMenuItem = ref<any>(null) // 右键菜单对应的条目

// 备注输入框引用
const noteInputRef = ref<HTMLInputElement | null>(null)

// 分组管理相关状态
const showGroupManager = ref(false) // 是否显示分组管理器
const groups = ref<Group[]>([]) // 分组列表
const showGroupForm = ref(false) // 是否显示分组表单
const editingGroup = ref<Group | null>(null) // 正在编辑的分组
const groupForm = ref({ name: '', color: '#3B82F6' }) // 分组表单数据
const showGroupSelector = ref(false) // 是否显示分组选择器
const selectedItemForGroup = ref<any>(null) // 要加入分组的条目

// 确认对话框相关状态
const showConfirmDialog = ref(false) // 是否显示确认对话框
const confirmDialog = ref({
  title: '',
  message: '',
  confirmText: '确定',
  cancelText: '取消',
  type: 'warning' as 'warning' | 'danger' | 'info',
  onConfirm: () => {}
})

// TypeScript 类型定义
interface Group {
  id: number
  name: string
  color: string
  created_at: string
  item_count: number
}
const allHistoryCache = shallowRef<any[]>([]) // 缓存全部数据

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
      triggerRef(clipboardHistory)
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
  
  // 尝试多种方式找到可见的搜索框
  let searchInput: HTMLInputElement | null = null
  
  // 方法1：使用 ref 引用
  if (searchInputRef.value && searchInputRef.value.offsetParent !== null) {
    searchInput = searchInputRef.value
    logger.debug('使用 ref 引用找到搜索框')
  }
  
  // 方法2：直接查找当前可见的搜索框
  if (!searchInput) {
    const allInputs = document.querySelectorAll('input[placeholder*="Search"]') as NodeListOf<HTMLInputElement>
    for (const input of allInputs) {
      // 检查输入框是否可见
      if (input.offsetParent !== null) {
        searchInput = input
        logger.debug('通过查询选择器找到搜索框')
        break
      }
    }
  }
  
  if (searchInput) {
    try {
      searchInput.focus()
      // 选中搜索框中的所有文本（如果有的话）
      if (searchInput.value) {
        searchInput.select()
      }
      
      // 验证是否真的获得了焦点
      const hasFocus = document.activeElement === searchInput
      logger.debug('搜索框聚焦结果', { 
        hasValue: !!searchInput.value,
        hasFocus,
        activeElement: document.activeElement?.tagName,
        placeholder: searchInput.placeholder
      })
      
      if (!hasFocus) {
        // 如果没有获得焦点，再试一次
        setTimeout(() => {
          searchInput?.focus()
          logger.debug('重试聚焦搜索框')
        }, 100)
      }
    } catch (error) {
      logger.error('聚焦搜索框失败', { error: String(error) })
    }
  } else {
    logger.warn('未找到可见的搜索框', {
      refExists: !!searchInputRef.value,
      refVisible: searchInputRef.value?.offsetParent !== null,
      selectedTab: selectedTabIndex.value
    })
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
  
  // 根据标签页筛选：0=All显示所有，1=Text只显示文本，2=Images只显示图片，3=Favorites只显示收藏的，4=分组
  let items: any[] = []
  
  if (selectedTabIndex.value === 0) {
    // 全部标签页：显示所有内容
    items = clipboardHistory.value
  } else if (selectedTabIndex.value === 1) {
    // 文本标签页：只显示文本类型的内容
    items = clipboardHistory.value.filter(item => item.type === 'text')
  } else if (selectedTabIndex.value === 2) {
    // 图片标签页：只显示图片类型的内容
    items = clipboardHistory.value.filter(item => item.type === 'image')
  } else if (selectedTabIndex.value === 3) {
    // 收藏标签页：只显示收藏的内容
    items = clipboardHistory.value.filter(item => item.isFavorite === true)
  } else if (selectedTabIndex.value === 4) {
    // 分组标签页：显示当前分组的内容
    items = clipboardHistory.value
  }
  
  // 应用搜索过滤
  const result = items.filter(item => {
    // 如果没有搜索查询，返回所有项目
    if (!query) return true
    
    // 根据当前标签页决定搜索逻辑
    if (selectedTabIndex.value === 2) {
      // 图片标签页：图片内容不支持文本搜索，返回false（搜索时不显示任何图片）
      return false
    } else {
      // 全部、文本、收藏和分组标签页：只搜索文本类型的内容
      if (item.type === 'text') {
        return item.content?.toLowerCase().includes(query) || false
      }
      return false
    }
  })
  
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
      
      // 更新全部数据缓存中的收藏状态
      if (allDataLoaded.value) {
        const cacheIndex = allHistoryCache.value.findIndex(i => i.id === item.id)
        if (cacheIndex !== -1) {
          allHistoryCache.value[cacheIndex] = { ...allHistoryCache.value[cacheIndex], isFavorite: newFavoriteStatus }
          triggerRef(allHistoryCache)
          logger.debug('更新全部数据缓存中的收藏状态', { itemId: item.id, newStatus: newFavoriteStatus })
        }
      }
      
      // 如果在收藏夹标签页取消收藏
      if (selectedTabIndex.value === 3 && !newFavoriteStatus) {
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

// 删除条目功能
const deleteItem = (item: any) => {
  const contentPreview = item.type === 'text' 
    ? (item.content.length > 20 ? item.content.substring(0, 20) + '...' : item.content)
    : '图片'
    
  showConfirm({
    title: '删除条目',
    message: `确定删除这个条目吗？\n内容: ${contentPreview}\n\n删除后无法恢复。`,
    confirmText: '删除',
    cancelText: '取消',
    type: 'danger',
    onConfirm: async () => {
      try {
        // 从数据库删除
        await db!.execute(
          'DELETE FROM clipboard_history WHERE id = ?',
          [item.id]
        )
        
        // 从内存中移除
        const index = clipboardHistory.value.findIndex(i => i.id === item.id)
        if (index !== -1) {
          clipboardHistory.value.splice(index, 1)
          triggerRef(clipboardHistory) // 触发 shallowRef 更新
        }
        
        // 从全部数据缓存中移除
        if (allDataLoaded.value) {
          const cacheIndex = allHistoryCache.value.findIndex(i => i.id === item.id)
          if (cacheIndex !== -1) {
            allHistoryCache.value.splice(cacheIndex, 1)
            triggerRef(allHistoryCache)
            logger.debug('从全部数据缓存中移除条目', { itemId: item.id })
          }
        }
        
        // 如果在搜索模式下，也从原始数据中移除
        if (isInSearchMode) {
          const originalIndex = originalClipboardHistory.findIndex(i => i.id === item.id)
          if (originalIndex !== -1) {
            originalClipboardHistory.splice(originalIndex, 1)
          }
        }
        
        // 清理缩略图缓存
        const itemKey = item.id.toString()
        if (thumbnailCache.value.has(itemKey)) {
          thumbnailCache.value.delete(itemKey)
          triggerRef(thumbnailCache)
        }
        
        // 如果当前选中的项是被删除的项，清除选中状态
        if (selectedItem.value?.id === item.id) {
          selectedItem.value = null
        }
        
        showSuccess('条目删除成功')
        logger.info('条目删除成功', { itemId: item.id, type: item.type })
      } catch (error) {
        showError('删除条目失败: ' + String(error))
        logger.error('删除条目失败', { itemId: item.id, error: String(error) })
      }
    }
  })
}

// 备注管理功能
const openNoteDialog = (item: any) => {
  editingNoteItem.value = item
  noteText.value = item.note || '' // 如果已有备注，显示现有备注
  showNoteDialog.value = true
  
  // 等待DOM更新后聚焦到输入框
  nextTick(() => {
    if (noteInputRef.value) {
      noteInputRef.value.focus()
      noteInputRef.value.select() // 如果有现有内容，全选
    }
  })
  
  logger.debug('打开备注编辑对话框', { itemId: item.id, hasExistingNote: !!item.note })
}

const closeNoteDialog = () => {
  showNoteDialog.value = false
  editingNoteItem.value = null
  noteText.value = ''
  logger.debug('关闭备注编辑对话框')
}

const saveNote = async () => {
  if (!editingNoteItem.value) return
  
  try {
    const trimmedNote = noteText.value.trim()
    
    // 调用后端API更新备注
    await invoke('update_item_note', { 
      itemId: editingNoteItem.value.id, 
      note: trimmedNote 
    })
    
    // 更新内存中的数据
    const updateItemNote = (item: any) => {
      if (item.id === editingNoteItem.value.id) {
        item.note = trimmedNote
      }
    }
    
    // 更新主列表
    clipboardHistory.value.forEach(updateItemNote)
    triggerRef(clipboardHistory)
    
    // 更新全部数据缓存
    if (allDataLoaded.value) {
      allHistoryCache.value.forEach(updateItemNote)
      triggerRef(allHistoryCache)
    }
    
    // 更新搜索结果（如果在搜索模式）
    if (isInSearchMode) {
      originalClipboardHistory.forEach(updateItemNote)
    }
    
    // 更新当前选中项
    if (selectedItem.value?.id === editingNoteItem.value.id) {
      selectedItem.value.note = trimmedNote
    }
    
    logger.info('备注保存成功', { 
      itemId: editingNoteItem.value.id, 
      noteLength: trimmedNote.length,
      hasNote: trimmedNote.length > 0
    })
    
    closeNoteDialog()
    showSuccess('备注保存成功')
  } catch (error) {
    logger.error('保存备注失败', { 
      itemId: editingNoteItem.value?.id, 
      error: String(error) 
    })
    showError('保存备注失败: ' + String(error))
  }
}

// 分组管理相关函数

// 加载分组列表
const loadGroups = async () => {
  try {
    const result = await invoke('get_groups') as Group[]
    groups.value = result
    logger.debug('加载分组列表成功', { count: result.length })
  } catch (error) {
    logger.error('加载分组列表失败', { error: String(error) })
    showError('加载分组列表失败: ' + String(error))
  }
}

// 创建或更新分组
const saveGroup = async () => {
  try {
    const { name, color } = groupForm.value
    if (!name.trim()) {
      showError('分组名称不能为空')
      return
    }

    if (editingGroup.value) {
      // 更新分组
      await invoke('update_group', {
        id: editingGroup.value.id,
        name: name.trim(),
        color
      })
      logger.info('分组更新成功', { id: editingGroup.value.id, name: name.trim() })
    } else {
      // 创建分组
      const newGroup = await invoke('create_group', {
        name: name.trim(),
        color
      }) as Group
      logger.info('分组创建成功', { id: newGroup.id, name: newGroup.name })
    }

    await loadGroups() // 重新加载分组列表
    closeGroupForm()
    showSuccess(editingGroup.value ? '分组更新成功' : '分组创建成功')
  } catch (error) {
    logger.error('保存分组失败', { error: String(error) })
    showError('保存分组失败: ' + String(error))
  }
}

// 删除分组
const deleteGroup = (group: Group) => {
  showConfirm({
    title: '删除分组',
    message: `确定删除分组 "${group.name}" 吗？\n该分组下的所有条目将移出分组。`,
    confirmText: '删除',
    cancelText: '取消',
    type: 'danger',
    onConfirm: async () => {
      try {
        await invoke('delete_group', { id: group.id })
        await loadGroups()
        logger.info('分组删除成功', { id: group.id, name: group.name })
        showSuccess('分组删除成功')
      } catch (error) {
        logger.error('删除分组失败', { error: String(error) })
        showError('删除分组失败: ' + String(error))
      }
    }
  })
}

// 打开分组表单
const openGroupForm = (group?: Group) => {
  editingGroup.value = group || null
  groupForm.value = {
    name: group?.name || '',
    color: group?.color || '#3B82F6'
  }
  showGroupForm.value = true
}

// 关闭分组表单
const closeGroupForm = () => {
  showGroupForm.value = false
  editingGroup.value = null
  groupForm.value = { name: '', color: '#3B82F6' }
}

// 打开分组选择器
const openGroupSelector = (item: any) => {
  selectedItemForGroup.value = item
  showGroupSelector.value = true
  loadGroups() // 确保分组列表是最新的
}

// 将条目加入分组
const addItemToGroup = async (groupId?: number) => {
  if (!selectedItemForGroup.value) return

  try {
    await invoke('add_item_to_group', {
      itemId: selectedItemForGroup.value.id,
      groupId: groupId || null
    })

    // 更新内存中的条目
    const item = selectedItemForGroup.value
    item.groupId = groupId || null

    // 更新缓存中的数据
    updateItemInCache(item)

    await loadGroups() // 重新加载分组列表以更新条目计数
    closeGroupSelector()
    
    const actionText = groupId ? '加入分组成功' : '移出分组成功'
    showSuccess(actionText)
    logger.info('条目分组设置成功', { itemId: item.id, groupId })
  } catch (error) {
    logger.error('设置条目分组失败', { error: String(error) })
    showError('设置条目分组失败: ' + String(error))
  }
}

// 关闭分组选择器
const closeGroupSelector = () => {
  showGroupSelector.value = false
  selectedItemForGroup.value = null
}

// 从分组中移除条目
const removeItemFromGroup = async (item: any) => {
  try {
    const currentGroup = groups.value.find(g => g.id === selectedGroupId.value)
    const groupName = currentGroup?.name || '未知分组'
    
    logger.debug('开始移除条目分组', { itemId: item.id, currentGroupId: selectedGroupId.value })
    
    // 调用后端API移除分组
    await invoke('add_item_to_group', {
      itemId: item.id,
      groupId: null // 设置为null表示移除分组
    })
    
    // 重新加载分组列表以更新条目数量
    await loadGroups()
    
    // 更新缓存中的条目
    updateItemInCache({ ...item, groupId: null })
    
    // 由于当前在分组模式下，移除分组后该条目将不再显示
    // 需要从当前显示的列表中移除
    const index = clipboardHistory.value.findIndex(i => i.id === item.id)
    if (index !== -1) {
      clipboardHistory.value.splice(index, 1)
      triggerRef(clipboardHistory)
    }
    
    // 如果移除的是当前选中的条目，清空选中状态
    if (selectedItem.value?.id === item.id) {
      selectedItem.value = null
    }
    
    logger.info('条目分组移除成功', { itemId: item.id, groupName })
    showSuccess(`已从"${groupName}"中移除`)
    
  } catch (error) {
    logger.error('移除条目分组失败', { itemId: item.id, error: String(error) })
    showError('移除分组失败: ' + String(error))
  }
}

// 更新缓存中的条目数据
const updateItemInCache = (updatedItem: any) => {
  // 更新主列表
  const mainIndex = clipboardHistory.value.findIndex(item => item.id === updatedItem.id)
  if (mainIndex !== -1) {
    clipboardHistory.value[mainIndex] = { ...clipboardHistory.value[mainIndex], ...updatedItem }
    triggerRef(clipboardHistory)
  }

  // 更新缓存
  const cacheIndex = allHistoryCache.value.findIndex(item => item.id === updatedItem.id)
  if (cacheIndex !== -1) {
    allHistoryCache.value[cacheIndex] = { ...allHistoryCache.value[cacheIndex], ...updatedItem }
    triggerRef(allHistoryCache)
  }
}

// 确认对话框辅助函数
const showConfirm = (options: {
  title: string
  message: string
  confirmText?: string
  cancelText?: string
  type?: 'warning' | 'danger' | 'info'
  onConfirm: () => void
}) => {
  confirmDialog.value = {
    title: options.title,
    message: options.message,
    confirmText: options.confirmText || '确定',
    cancelText: options.cancelText || '取消',
    type: options.type || 'warning',
    onConfirm: options.onConfirm
  }
  showConfirmDialog.value = true
}

const handleConfirmDialogConfirm = () => {
  confirmDialog.value.onConfirm()
}

// 分组过滤相关函数
const switchToGroup = async (groupId: number) => {
  const group = groups.value.find(g => g.id === groupId)
  if (!group) return
  
  const switchStart = performance.now()
  const fromTab = selectedGroupId.value 
    ? `分组(${groups.value.find(g => g.id === selectedGroupId.value)?.name || '未知'})`
    : `${selectedTabIndex.value}(${['全部', '文本', '图片', '收藏'][selectedTabIndex.value]})`
    
  logger.info('开始切换到分组', { 
    from: fromTab,
    to: `分组(${group.name})`,
    groupId: groupId,
    timestamp: new Date().toISOString()
  })
  
  selectedTabIndex.value = 4 // 设置为分组模式的虚拟索引
  selectedGroupId.value = groupId
  showGroupDropdown.value = false
  
  // 重置搜索和选中状态
  searchQuery.value = ''
  selectedItem.value = null
  
  // 重置分页状态
  currentOffset.value = 0
  hasMoreData.value = true
  
  // 如果在搜索模式，先退出搜索模式
  if (isInSearchMode) {
    logger.info('退出搜索模式')
    await exitSearchMode()
  } else {
    // 从全部数据缓存中过滤分组数据
    await loadGroupData(groupId)
  }
  
  const switchTime = performance.now() - switchStart
  logger.info('分组切换完成', { 
    totalTime: `${switchTime.toFixed(2)}ms`,
    newGroup: `分组(${group.name})`
  })
  
  // 切换后自动聚焦搜索框
  focusSearchInput()
}

const loadGroupData = async (groupId: number) => {
  logger.info('开始加载分组数据', { groupId })
  
  // 始终从数据库查询分组数据，避免仅使用内存缓存导致展示不全
  try {
    const rows = await db!.select(
      `SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon, thumbnail_data, note, group_id 
       FROM clipboard_history 
       WHERE group_id = ? 
       ORDER BY timestamp DESC 
       LIMIT ?`,
      [groupId, MAX_MEMORY_ITEMS]
    )
    
    const processStart = performance.now()
    const groupItems = rows.map((row: any) => ({
      id: row.id,
      content: row.content,
      type: row.type,
      timestamp: row.timestamp,
      isFavorite: row.is_favorite === 1,
      imagePath: row.image_path ?? null,
      sourceAppName: row.source_app_name ?? 'Unknown',
      sourceAppIcon: row.source_app_icon ?? null,
      note: row.note ?? null,
      groupId: row.group_id ?? null
    }))
    
    clipboardHistory.value = groupItems
    triggerRef(clipboardHistory)
    
    // 初始化分页状态以支持“加载更多”
    currentOffset.value = groupItems.length
    hasMoreData.value = groupItems.length >= MAX_MEMORY_ITEMS
    selectedItem.value = null
    
    const processTime = performance.now() - processStart
    logger.info('数据库分组查询完成', { 
      groupId, 
      queryTime: `${processTime.toFixed(2)}ms`, 
      rowCount: groupItems.length 
    })
  } catch (error) {
    logger.error('加载分组数据失败', { groupId, error: String(error) })
    showError('加载分组数据失败: ' + String(error))
  }
}

// 右键菜单管理功能
const showItemContextMenu = (event: MouseEvent, item: any) => {
  event.preventDefault()
  event.stopPropagation()
  
  contextMenuItem.value = item
  contextMenuPosition.value = {
    x: event.clientX,
    y: event.clientY
  }
  showContextMenu.value = true
  
  logger.debug('显示条目右键菜单', { itemId: item.id, x: event.clientX, y: event.clientY })
}

const hideContextMenu = () => {
  showContextMenu.value = false
  contextMenuItem.value = null
  logger.debug('隐藏右键菜单')
}

// 隐藏分组下拉菜单
const hideGroupDropdown = (event?: Event) => {
  if (event && (event.target as Element).closest('.relative')) {
    return // 如果点击的是分组按钮或下拉菜单内部，不隐藏
  }
  showGroupDropdown.value = false
}

const handleContextMenuAction = (action: string) => {
  if (!contextMenuItem.value) return
  
  const item = contextMenuItem.value
  logger.debug('执行右键菜单操作', { action, itemId: item.id })
  
  switch (action) {
    case 'note':
      openNoteDialog(item)
      break
    case 'group':
      openGroupSelector(item)
      break
    case 'remove-group':
      removeItemFromGroup(item)
      break
    case 'favorite':
      toggleFavorite(item)
      break
    case 'delete':
      deleteItem(item)
      break
    case 'copy':
      copyToClipboard(item)
      break
  }
  
  hideContextMenu()
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
        'SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon, thumbnail_data, note, group_id FROM clipboard_history WHERE id = ?',
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
          sourceAppIcon: row.source_app_icon ?? null,
          note: row.note ?? null,
          groupId: row.group_id ?? null
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

// 生成状态记录，防止重复生成
const generatingThumbnails = ref(new Set<string>())

// 可视区域计算
// const calculateVisibleItems = (scrollTop: number, containerHeight: number, itemHeight: number) => {
//   const startIndex = Math.floor(scrollTop / itemHeight)
//   const endIndex = Math.min(
//     startIndex + Math.ceil(containerHeight / itemHeight) + 5, // 额外渲柕 5 个项目
//     filteredHistory.value.length - 1
//   )
//   return { startIndex: Math.max(0, startIndex - 2), endIndex } // 预渲柕 2 个项目
// }

// 仅为新复制的图片生成缩略图
const generateThumbnailForNewItem = async (item: any) => {
  if (item.type !== 'image') {
    return
  }
  
  const itemKey = item.id.toString()
  
  // 检查是否已经在缓存中或正在生成
  if (thumbnailCache.value.has(itemKey) || generatingThumbnails.value.has(itemKey)) {
    logger.debug('缩略图已存在或正在生成，跳过', { itemId: item.id })
    return
  }
  
  // 标记为正在生成
  generatingThumbnails.value.add(itemKey)
  
  try {
    let originalImage = item.content
    
    // 如果有imagePath（旧格式），优先使用
    if (item.imagePath) {
      originalImage = await invoke('load_image_file', { imagePath: item.imagePath }) as string
    }
    
    logger.debug('开始为新图片生成缩略图', { itemId: item.id, hasOriginalImage: !!originalImage })
    
    // 生成缩略图
    const thumbnail = await invoke('generate_thumbnail', { 
      base64Data: originalImage,
      width: 200,
      height: 150
    }) as string
    
    // 存入缓存
    thumbnailCache.value.set(itemKey, thumbnail)
    triggerRef(thumbnailCache) // 手动触发缓存更新
    
    // 将缩略图保存到数据库
    try {
      await db!.execute(
        'UPDATE clipboard_history SET thumbnail_data = ? WHERE id = ?',
        [thumbnail, item.id]
      )
      logger.debug('缩略图已保存到数据库', { itemId: item.id })
    } catch (dbError) {
      logger.warn('保存缩略图到数据库失败', { itemId: item.id, error: String(dbError) })
    }
    
    logger.debug('新图片缩略图生成成功', { itemId: item.id })
  } catch (error) {
    logger.warn('生成缩略图失败', { error: String(error), itemId: item.id })
    // 失败时使用原图作为后备
    thumbnailCache.value.set(itemKey, item.content || '')
    triggerRef(thumbnailCache) // 手动触发缓存更新
  } finally {
    // 移除生成状态标记
    generatingThumbnails.value.delete(itemKey)
  }
}

// 检查项目是否在可视区域内
// const isItemVisible = (itemIndex: number, scrollContainer?: HTMLElement): boolean => {
//   if (!scrollContainer) return true
//   
//   const itemHeight = 80 // 估算的项目高度
//   const itemTop = itemIndex * itemHeight
//   const itemBottom = itemTop + itemHeight
//   
//   const containerTop = scrollContainer.scrollTop
//   const containerBottom = containerTop + scrollContainer.clientHeight
//   
//   // 添加一些缓冲区域
//   const buffer = 200
//   return itemBottom >= (containerTop - buffer) && itemTop <= (containerBottom + buffer)
// }

// 统计缩略图调用次数
let thumbnailCallCount = 0
let lastThumbnailLogTime = 0

// 获取缩略图（同步，用于模板）
const getThumbnailSync = (item: any): string | undefined => {
  if (item.type !== 'image') {
    return undefined
  }
  
  thumbnailCallCount++
  const now = Date.now()
  
  // 每秒最多记录一次日志，避免日志洪水
  if (now - lastThumbnailLogTime > 1000) {
    logger.debug('缩略图调用统计', { 
      callCount: thumbnailCallCount,
      cacheSize: thumbnailCache.value.size,
      recentItemId: item.id
    })
    lastThumbnailLogTime = now
    thumbnailCallCount = 0
  }
  
  const itemKey = item.id.toString()
  
  // 如果缓存中有，直接返回
  if (thumbnailCache.value.has(itemKey)) {
    return thumbnailCache.value.get(itemKey)!
  }

  // 对于历史数据，不生成缩略图，返回 undefined 以显示占位符
  return undefined
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

// 跨平台快捷键检测工具函数
const isMac = () => navigator.platform.toLowerCase().includes('mac')
const getModifierKey = () => isMac() ? 'Cmd' : 'Ctrl'
const isModifierPressed = (e: KeyboardEvent) => isMac() ? e.metaKey : e.ctrlKey

const handleKeyDown = async (e: KeyboardEvent) => {
  // 处理搜索快捷键，聚焦到搜索框（跨平台支持）
  // Windows/Linux: Ctrl+F, macOS: Cmd+F
  const isSearchShortcut = (e.key === 'f' || e.key === 'F') && isModifierPressed(e)
  
  if (isSearchShortcut) {
    e.preventDefault()
    e.stopPropagation()
    logger.debug(`${getModifierKey()}+F 快捷键被触发，聚焦搜索框`)
    await focusSearchInput()
    return
  }
  
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
    // 向左切换标签页
    const currentTab = selectedTabIndex.value
    const newTab = currentTab > 0 ? currentTab - 1 : 3 // 循环切换：0 -> 3, 1 -> 0, 2 -> 1, 3 -> 2
    await switchTab(newTab)
    return
  } else if (e.key === 'ArrowRight') {
    e.preventDefault()
    // 向右切换标签页
    const currentTab = selectedTabIndex.value
    const newTab = currentTab < 3 ? currentTab + 1 : 0 // 循环切换：0 -> 1, 1 -> 2, 2 -> 3, 3 -> 0
    await switchTab(newTab)
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

// 处理按钮切换
// 智能加载标签页数据：优先使用内存缓存
const loadTabData = async (tabIndex: number) => {
  const isTextTab = tabIndex === 1
  const isImagesTab = tabIndex === 2
  const isFavoritesTab = tabIndex === 3
  const isAllTab = tabIndex === 0
  
  // 如果是“全部”标签页，或者还没有加载过全部数据，则从数据库加载
  if (isAllTab || !allDataLoaded.value) {
    logger.info('从数据库加载数据', { 
      reason: isAllTab ? '全部标签页' : '未加载过全部数据',
      tabIndex 
    })
    await loadRecentHistory()
    
    // 如果是“全部”标签页，缓存数据
    if (isAllTab) {
      allHistoryCache.value = [...clipboardHistory.value]
      allDataLoaded.value = true
      logger.info('已缓存全部数据', { count: allHistoryCache.value.length })
    }
  } else {
    // 收藏标签页改为使用数据库查询，确保展示数据库中的所有收藏并支持分页
    if (isFavoritesTab) {
      logger.info('收藏标签页使用数据库查询以覆盖内存过滤', { tabIndex })
      await loadRecentHistory()
      return
    }

    // 使用内存中的数据进行过滤（文本/图片）
    logger.info('使用内存数据进行过滤', { 
      tabIndex,
      cacheSize: allHistoryCache.value.length 
    })
    
    let filteredData: any[] = []
    
    if (isTextTab) {
      filteredData = allHistoryCache.value.filter(item => item.type === 'text')
    } else if (isImagesTab) {
      filteredData = allHistoryCache.value.filter(item => item.type === 'image')
    }
    
    // 直接设置过滤后的数据
    clipboardHistory.value = filteredData
    triggerRef(clipboardHistory)
    
    // 设置分页状态
    currentOffset.value = filteredData.length
    hasMoreData.value = false // 内存过滤不需要分页
    selectedItem.value = null
    
    logger.info('内存过滤完成', { 
      tabType: isTextTab ? 'text' : isImagesTab ? 'image' : 'favorites',
      filteredCount: filteredData.length,
      totalCount: allHistoryCache.value.length
    })
  }
}

// 禁用浏览器原生右键菜单（只对非条目区域生效）
const preventDefaultContextMenu = (e: MouseEvent) => {
  e.preventDefault()
  e.stopPropagation()
  return false
}

const switchTab = async (index: number) => {
  if (selectedTabIndex.value === index && selectedGroupId.value === null) return // 如果已经是当前tab且不是分组模式，不需要切换
  
  const switchStart = performance.now()
  const tabNames = ['全部', '文本', '图片', '收藏']
  const fromTab = selectedGroupId.value 
    ? `分组(${groups.value.find(g => g.id === selectedGroupId.value)?.name || '未知'})`
    : `${selectedTabIndex.value}(${tabNames[selectedTabIndex.value]})`
  logger.info('开始切换标签页', { 
    from: fromTab,
    to: `${index}(${tabNames[index]})`,
    timestamp: new Date().toISOString()
  })
  
  selectedTabIndex.value = index
  selectedGroupId.value = null // 切换普通标签时清除分组选择
  // 重置搜索和选中状态
  searchQuery.value = ''
  selectedItem.value = null
  
  // 重置分页状态（将在 loadRecentHistory 中设置正确的值）
  currentOffset.value = 0
  hasMoreData.value = true
  
  // 如果在搜索模式，先退出搜索模式
  if (isInSearchMode) {
    logger.info('退出搜索模式')
    await exitSearchMode()
  } else {
    // 智能加载数据：优先使用内存中的数据
    logger.info('开始加载标签页数据')
    await loadTabData(index)
  }
  
  const switchTime = performance.now() - switchStart
  logger.info('标签页切换完成', { 
    totalTime: `${switchTime.toFixed(2)}ms`,
    newTab: `${index}(${tabNames[index]})`
  })
  
  // 切换标签页后自动聚焦搜索框
  focusSearchInput()
}


// 按需加载图片内容
const loadImageContent = async (item: any): Promise<string | null> => {
  if (item.type !== 'image') return null
  
  try {
    // 如果已经有内容且不是空字符串，直接返回
    if (item.content && item.content.trim() !== '') {
      return item.content
    }
    
    // 从数据库加载完整内容
    logger.info('按需加载图片内容', { itemId: item.id })
    const loadStart = performance.now()
    
    const rows = await db!.select(
      'SELECT content FROM clipboard_history WHERE id = ?',
      [item.id]
    )
    
    const loadTime = performance.now() - loadStart
    
    if (rows.length > 0) {
      const content = rows[0].content
      logger.info('图片内容加载完成', { 
        itemId: item.id, 
        loadTime: `${loadTime.toFixed(2)}ms`,
        contentSize: content ? `${(content.length / 1024).toFixed(1)}KB` : '0KB'
      })
      return content
    }
    
    return null
  } catch (error) {
    logger.error('加载图片内容失败', { itemId: item.id, error: String(error) })
    return null
  }
}

// 监听选中项变化，当选中图片时加载完整图片
watch(selectedItem, async (newItem) => {
  // 清理之前的图片内容，释放内存
  if (fullImageContent.value) {
    fullImageContent.value = null
  }
  
  if (newItem && newItem.type === 'image') {
    try {
      let imageContent: string | null = null
      
      // 优先尝试从 imagePath 加载（兼容旧数据）
      if (newItem.imagePath) {
        logger.info('从文件路径加载图片', { imagePath: newItem.imagePath })
        imageContent = await invoke('load_image_file', { imagePath: newItem.imagePath }) as string
        } else {
        // 使用按需加载函数
        imageContent = await loadImageContent(newItem)
        }
      
      if (imageContent) {
          // 检查图片大小
        if (imageContent.length > MAX_IMAGE_PREVIEW_SIZE) {
          logger.warn('图片过大，但仍然显示', { 
            itemId: newItem.id,
            size: `${(imageContent.length / 1024 / 1024).toFixed(1)}MB`
          })
        }
        fullImageContent.value = imageContent
        } else {
        logger.warn('无法加载图片内容', { itemId: newItem.id })
          fullImageContent.value = null
      }
    } catch (error) {
      logger.error('加载图片失败', { itemId: newItem.id, error: String(error) })
      fullImageContent.value = null
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
    const isTextTab = selectedTabIndex.value === 1
    const isImagesTab = selectedTabIndex.value === 2
    const isFavoritesTab = selectedTabIndex.value === 3
    
    // 图片标签页不支持搜索，直接返回空结果
    if (isImagesTab) {
      clipboardHistory.value = []
      selectedItem.value = null
      logger.debug('图片标签页不支持搜索')
      return
    }
    
    // 构建SQL查询 - 只搜索文本类型的内容
    let sql = `
      SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon, note, group_id 
      FROM clipboard_history 
      WHERE type = 'text' AND LOWER(content) LIKE ?
    `
    
    const params = [`%${query}%`]
    
    // 根据不同标签页添加额外条件
    if (isTextTab) {
      // 文本标签页：已经通过 type = 'text' 过滤了
    } else if (isFavoritesTab) {
      // 收藏标签页：只搜索收藏的文本项目
      sql += ' AND is_favorite = 1'
    } else if (selectedTabIndex.value === 4 && selectedGroupId.value !== null) {
      // 分组标签页：只搜索当前分组下的文本项目
      sql += ' AND group_id = ?'
      params.push(selectedGroupId.value as any)
    }
    // 全部标签页：搜索所有文本内容（无额外条件）
    
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
        sourceAppIcon: row.source_app_icon ?? null,
        note: row.note ?? null,
        groupId: row.group_id ?? null
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
    const isTextTab = selectedTabIndex.value === 1
    const isImagesTab = selectedTabIndex.value === 2
    const isFavoritesTab = selectedTabIndex.value === 3
    const isGroupTab = selectedTabIndex.value === 4 && selectedGroupId.value !== null
    
    let sql = `
      SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon, thumbnail_data, note, group_id 
      FROM clipboard_history
    `
    
    if (isTextTab) {
      sql += ' WHERE type = \'text\''
    } else if (isImagesTab) {
      sql += ' WHERE type = \'image\''
      // 对于图片标签页，不加载完整的 content 字段
      sql = sql.replace('content', '\'\' as content')
    } else if (isFavoritesTab) {
      sql += ' WHERE is_favorite = 1'
    } else if (isGroupTab) {
      sql += ' WHERE group_id = ?'
    }
    
    sql += ' ORDER BY timestamp DESC LIMIT ? OFFSET ?'
    
    const params = isGroupTab 
      ? [selectedGroupId.value, 50, currentOffset.value]
      : [50, currentOffset.value]
    
    const rows = await db.select(sql, params)
    
    if (rows.length === 0) {
      hasMoreData.value = false
      logger.debug('没有更多数据了')
      return
    }
    
    // 获取已有的ID集合以防止重复
    const existingIds = new Set(clipboardHistory.value.map(item => item.id))
    
    const newItems = rows
      .filter((row: any) => !existingIds.has(row.id)) // 过滤掉已存在的记录
      .map((row: any) => {
        const item = {
      id: row.id,
      content: row.content,
      type: row.type,
      timestamp: row.timestamp,
      isFavorite: row.is_favorite === 1,
      imagePath: row.image_path ?? null,
      sourceAppName: row.source_app_name ?? 'Unknown',
      sourceAppIcon: row.source_app_icon ?? null,
      note: row.note ?? null,
      groupId: row.group_id ?? null
        }
        
        // 如果是图片且有缩略图数据，恢复到缓存中
        if (row.type === 'image' && row.thumbnail_data) {
          const itemKey = row.id.toString()
          thumbnailCache.value.set(itemKey, row.thumbnail_data)
          logger.debug('从数据库恢复缩略图（加载更多）', { itemId: row.id })
        }
        
        return item
      })
    
    // 追加新记录到历史列表
    if (newItems.length > 0) {
    clipboardHistory.value.push(...newItems)
      triggerRef(clipboardHistory) // 触发 shallowRef 更新
    }
    currentOffset.value += rows.length // 使用原始查询的数据量来更新偏移量
    
    // 如果恢复了缩略图，触发缓存更新
    const thumbnailsRestored = newItems.filter((item: any) => item.type === 'image' && thumbnailCache.value.has(item.id.toString())).length
    if (thumbnailsRestored > 0) {
      triggerRef(thumbnailCache)
    }
    
    logger.debug('加载了更多记录', { 
      queriedCount: rows.length,
      newItemsCount: newItems.length,
      duplicatesFiltered: rows.length - newItems.length,
      totalCount: clipboardHistory.value.length,
      currentOffset: currentOffset.value,
      thumbnailsRestored 
    })
    
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
  
  const startTime = performance.now()
  logger.info('开始加载历史记录', { 
    selectedTab: selectedTabIndex.value,
    timestamp: new Date().toISOString()
  })
  
  try {
    const isTextTab = selectedTabIndex.value === 1
    const isImagesTab = selectedTabIndex.value === 2
    const isFavoritesTab = selectedTabIndex.value === 3
    const isGroupTab = selectedTabIndex.value === 4 && selectedGroupId.value !== null
    
    let sql: string
    
    // 对于图片标签页，不加载完整的 content 字段以提高性能，但加载缩略图数据
    if (isImagesTab) {
      sql = `
        SELECT id, '' as content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon, thumbnail_data, note, group_id 
        FROM clipboard_history
        WHERE type = 'image'
        ORDER BY timestamp DESC LIMIT ?
      `
      logger.info('使用优化的图片查询（不加载 content 字段，但加载缩略图）')
    } else {
      sql = `
        SELECT id, content, type, timestamp, is_favorite, image_path, source_app_name, source_app_icon, thumbnail_data, note, group_id 
      FROM clipboard_history
    `
    
      if (isTextTab) {
        sql += ' WHERE type = \'text\''
      } else if (isFavoritesTab) {
      sql += ' WHERE is_favorite = 1'
    } else if (isGroupTab) {
      sql += ' WHERE group_id = ?'
    }
    
    sql += ' ORDER BY timestamp DESC LIMIT ?'
    }
    
    const dbQueryStart = performance.now()
    const params = isGroupTab ? [selectedGroupId.value, MAX_MEMORY_ITEMS] : [MAX_MEMORY_ITEMS]
    const rows = await db.select(sql, params)
    const dbQueryTime = performance.now() - dbQueryStart
    logger.info('数据库查询完成', { 
      queryTime: `${dbQueryTime.toFixed(2)}ms`,
      rowCount: rows.length,
      tabType: isTextTab ? 'text' : isImagesTab ? 'image' : isFavoritesTab ? 'favorites' : isGroupTab ? 'group' : 'all'
    })
    
    // 确保去重
    const processStart = performance.now()
    const seenIds = new Set()
    const deduplicatedHistory = rows
      .map((row: any) => {
        const item = {
        id: row.id,
          content: row.content, // 对于图片标签页，这里是空字符串
        type: row.type,
        timestamp: row.timestamp,
        isFavorite: row.is_favorite === 1,
        imagePath: row.image_path ?? null,
        sourceAppName: row.source_app_name ?? 'Unknown',
          sourceAppIcon: row.source_app_icon ?? null,
          note: row.note ?? null,
          groupId: row.group_id ?? null,
          // 标记是否需要懒加载内容
          needsContentLoad: isImagesTab && row.type === 'image'
        }
        
        // 如果是图片且有缩略图数据，恢复到缓存中
        if (row.type === 'image' && row.thumbnail_data) {
          const itemKey = row.id.toString()
          thumbnailCache.value.set(itemKey, row.thumbnail_data)
          logger.debug('从数据库恢复缩略图', { itemId: row.id })
        }
        
        return item
      })
      .filter((item: any) => {
        if (seenIds.has(item.id)) {
          return false
        }
        seenIds.add(item.id)
        return true
      })
    
    const processTime = performance.now() - processStart
    logger.info('数据处理完成', { 
      processTime: `${processTime.toFixed(2)}ms`,
      originalCount: rows.length,
      deduplicatedCount: deduplicatedHistory.length,
      thumbnailsRestored: thumbnailCache.value.size
    })
    
    // 如果恢复了缩略图，触发缓存更新
    if (thumbnailCache.value.size > 0) {
      triggerRef(thumbnailCache)
    }
    
    const updateStart = performance.now()
    // 使用 shallowRef 时需要手动触发更新
    clipboardHistory.value = deduplicatedHistory
    triggerRef(clipboardHistory)
    
    // 重置分页状态和选中状态
    currentOffset.value = deduplicatedHistory.length
    hasMoreData.value = deduplicatedHistory.length >= MAX_MEMORY_ITEMS // 只有加载了满的数据才可能有更多
    selectedItem.value = null
    
    // 如果加载的是“全部”数据，更新缓存
    if (selectedTabIndex.value === 0) {
      allHistoryCache.value = [...deduplicatedHistory]
      allDataLoaded.value = true
      logger.debug('更新全部数据缓存', { count: allHistoryCache.value.length })
    }
    
    const updateTime = performance.now() - updateStart
    const totalTime = performance.now() - startTime
    
    logger.info('历史记录加载完成', { 
      totalTime: `${totalTime.toFixed(2)}ms`,
      updateTime: `${updateTime.toFixed(2)}ms`,
      totalCount: clipboardHistory.value.length,
      breakdown: {
        dbQuery: `${dbQueryTime.toFixed(2)}ms`,
        dataProcess: `${processTime.toFixed(2)}ms`,
        vueUpdate: `${updateTime.toFixed(2)}ms`
      }
    })
    
    // 历史数据不生成缩略图，只有新复制的图片才生成
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
    
    // 初始加载分组数据
    await loadGroups()

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
            triggerRef(clipboardHistory)
            
            // 新数据加入，需要失效缓存
            if (allDataLoaded.value) {
              allHistoryCache.value.unshift(newItem)
              triggerRef(allHistoryCache)
              logger.debug('更新全部数据缓存，添加新条目', { itemId: newItem.id })
            }
            
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
            triggerRef(clipboardHistory)
            
            // 新数据加入，需要失效缓存
            if (allDataLoaded.value) {
              allHistoryCache.value.unshift(newItem)
              triggerRef(allHistoryCache)
              logger.debug('更新全部数据缓存，添加新图片', { itemId: newItem.id })
            }
            
            // 为新复制的图片生成缩略图
            generateThumbnailForNewItem(newItem)
            
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
    
    // 禁用浏览器原生右键菜单
    document.addEventListener('contextmenu', preventDefaultContextMenu)
    logger.debug('已禁用浏览器原生右键菜单')
    
    // 点击外部隐藏右键菜单
    document.addEventListener('click', hideContextMenu)
    
    // 点击外部隐藏分组下拉菜单
    document.addEventListener('click', hideGroupDropdown)
    
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
  
  // 清理右键菜单事件监听器
  document.removeEventListener('contextmenu', preventDefaultContextMenu)
  document.removeEventListener('click', hideContextMenu)
  
  // 清理分组下拉菜单事件监听器
  document.removeEventListener('click', hideGroupDropdown)
  
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

// 监听 clipboardHistory 变化，记录 DOM 更新时间
watch(clipboardHistory, async (newHistory, oldHistory) => {
  if (newHistory.length !== oldHistory?.length) {
    const updateStart = performance.now()
    await nextTick() // 等待 DOM 更新完成
    const updateTime = performance.now() - updateStart
    
    logger.info('DOM更新完成', {
      domUpdateTime: `${updateTime.toFixed(2)}ms`,
      itemCount: newHistory.length,
      oldCount: oldHistory?.length || 0
    })
  }
}, { deep: false }) // 不深度监听，只监听数组本身的变化



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





</script>

<template>
  <div class="h-screen flex flex-col bg-gray-50 rounded-lg shadow-2xl border border-gray-200 overflow-hidden">
    <!-- Main Content -->
    <div class="flex-1 flex min-h-0">
      <!-- Left Sidebar -->
      <div class="w-96 lg:w-[28rem] bg-white border-r border-gray-200 flex flex-col min-h-0 shadow-sm">
        <div class="flex flex-col h-full">
          <!-- Search Bar (moved to top) -->
          <div class="p-2 border-b border-gray-100 flex-shrink-0">
            <div class="relative">
              <input
                v-model="searchQuery"
                type="text"
                :placeholder="searchPlaceholders[selectedTabIndex]"
                class="w-full pl-8 pr-3 py-2 border border-gray-200 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all duration-200 text-xs"
                ref="searchInputRef"
              />
              <div class="absolute inset-y-0 left-0 pl-2.5 flex items-center pointer-events-none">
                <MagnifyingGlassIcon class="h-3.5 w-3.5 text-gray-400" />
      </div>
      </div>
    </div>

          <!-- Navigation Buttons (moved below search) -->
          <div class="flex-shrink-0 bg-white px-4 py-1 border-b border-gray-200">
            <div class="flex items-center justify-center space-x-2 max-w-[320px] mx-auto">
              <!-- 全部按钮 -->
                <button
                @click="switchTab(0)"
                class="clean-nav-button px-3 py-1 text-xs rounded focus:outline-none min-w-[50px]"
                  :class="[
                  selectedTabIndex === 0
                    ? 'text-white bg-blue-500'
                    : 'text-gray-600 hover:text-gray-800 hover:bg-gray-100'
                ]"
              >
                全部
                </button>
              <!-- 文本按钮 -->
                <button
                @click="switchTab(1)"
                class="clean-nav-button px-3 py-1 text-xs rounded focus:outline-none min-w-[50px]"
                  :class="[
                  selectedTabIndex === 1
                    ? 'text-white bg-blue-500'
                    : 'text-gray-600 hover:text-gray-800 hover:bg-gray-100'
                ]"
              >
                文本
                </button>
              <!-- 图片按钮 -->
          <button 
                @click="switchTab(2)"
                class="clean-nav-button px-3 py-1 text-xs rounded focus:outline-none min-w-[50px]"
                :class="[
                  selectedTabIndex === 2
                    ? 'text-white bg-blue-500'
                    : 'text-gray-600 hover:text-gray-800 hover:bg-gray-100'
                ]"
              >
                图片
          </button>
              <!-- 收藏按钮 -->
          <button 
                @click="switchTab(3)"
                class="clean-nav-button px-3 py-1 text-xs rounded focus:outline-none min-w-[50px]"
                :class="[
                  selectedTabIndex === 3
                    ? 'text-white bg-blue-500'
                    : 'text-gray-600 hover:text-gray-800 hover:bg-gray-100'
                ]"
              >
                收藏
          </button>
              
              <!-- 分组按钮 -->
              <div class="relative">
                <button
                  @click="availableGroups.length > 0 ? (showGroupDropdown = !showGroupDropdown) : null"
                  class="clean-nav-button px-3 py-1 text-xs rounded focus:outline-none min-w-[50px] flex items-center space-x-1"
                  :class="[
                    selectedTabIndex === 4 
                      ? 'text-white bg-blue-500' 
                      : availableGroups.length > 0 
                        ? 'text-gray-600 hover:text-gray-800 hover:bg-gray-100'
                        : 'text-gray-400 cursor-not-allowed'
                  ]"
                >
                  <span>分组</span>
                  <svg 
                    v-if="availableGroups.length > 0"
                    class="w-3 h-3 transition-transform duration-200" 
                    :class="{ 'rotate-180': showGroupDropdown }"
                    fill="none" 
                    stroke="currentColor" 
                    viewBox="0 0 24 24"
                  >
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"></path>
                    </svg>
                  <span v-else class="text-xs text-gray-400">({{ availableGroups.length }})</span>
                </button>
                
                <!-- 分组下拉菜单 -->
                <div 
                  v-if="showGroupDropdown && availableGroups.length > 0"
                  class="absolute top-full left-0 mt-1 bg-white border border-gray-200 rounded-md z-50 py-0.5 min-w-[120px]"
                  @click.stop
                >
                <button
                    v-for="group in availableGroups"
                    :key="group.id"
                    @click="switchToGroup(group.id)"
                    class="w-full px-2 py-1 text-left text-xs text-gray-700 hover:bg-gray-100 transition-colors flex items-center space-x-2"
                  >
                    <div 
                      class="w-2.5 h-2.5 rounded-full flex-shrink-0"
                      :style="{ backgroundColor: group.color }"
                    ></div>
                    <span class="truncate">{{ group.name }}</span>
                    <span class="text-gray-400 text-xs ml-auto">({{ group.item_count }})</span>
                </button>
          </div>
                  </div>
                </div>
              </div>

          <div class="flex-1 min-h-0">
            <div class="h-full flex flex-col min-h-0">
              <!-- 分组头部信息 (仅在分组模式下显示) -->
              <div v-if="currentTabInfo.showGroupHeader" class="px-3 py-2 bg-blue-50 border-b border-blue-100">
                <div class="flex items-center space-x-2">
                  <div 
                    class="w-3 h-3 rounded-full"
                    :style="{ backgroundColor: groups.find(g => g.id === selectedGroupId)?.color || '#3B82F6' }"
                  ></div>
                  <span class="text-sm font-medium text-blue-700">
                    {{ groups.find(g => g.id === selectedGroupId)?.name || '未知分组' }}
                  </span>
                  <span class="text-xs text-blue-500">({{ clipboardHistory.length }} 个条目)</span>
                </div>
              </div>
              
              <!-- 统一的列表内容 -->
              <div class="flex-1 overflow-y-auto min-h-0" @scroll="handleScroll">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  :data-item-id="item.id"
                  :title="item.note || ''"
                  class="group px-3 py-2 border-b border-gray-50 hover:bg-blue-50 cursor-pointer transition-all duration-200"
                  :class="{ 
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id && selectedItem?.id !== undefined,
                    'hover:bg-gray-50': selectedItem?.id !== item.id || selectedItem?.id === undefined
                  }"
                  @click="selectedItem = item"
                  @dblclick="handleDoubleClick(item)"
                  @contextmenu="showItemContextMenu($event, item)"
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
                            <!-- 备注指示器 -->
                            <div 
                              v-if="item.note"
                              class="w-1.5 h-1.5 bg-blue-400 rounded-full"
                              :title="item.note"
                            ></div>
                        </div>
                        <div v-if="item.type === 'text'" class="text-xs text-gray-900 line-clamp-2 leading-snug">
                          {{ item.content }}
                      </div>
                        <div v-else class="mt-1">
                          <img 
                            v-if="getThumbnailSync(item)"
                            :src="getThumbnailSync(item)"
                            alt="Image thumbnail"
                            class="w-16 h-12 object-cover rounded border"
                            loading="lazy"
                            @error="($event.target as HTMLImageElement).style.display = 'none'"
                          />
                          <div 
                            v-else
                            class="w-16 h-12 bg-gray-100 rounded border flex items-center justify-center"
                          >
                            <svg class="w-6 h-6 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"></path>
                            </svg>
                    </div>
                        </div>
                      </div>
                    </div>
                    <div class="flex items-center space-x-1">
                      <!-- 删除按钮 -->
                      <button
                        class="flex-shrink-0 p-0.5 text-gray-400 hover:text-red-500 transition-colors duration-200 opacity-0 group-hover:opacity-100"
                        @click.stop="deleteItem(item)"
                        title="删除"
                      >
                        <TrashIcon class="w-3.5 h-3.5" />
                      </button>
                      <!-- 收藏按钮 -->
                    <button
                      class="flex-shrink-0 p-0.5 text-gray-400 hover:text-yellow-500 transition-colors duration-200 opacity-0 group-hover:opacity-100"
                      :class="{ 'opacity-100': item.isFavorite }"
                      @click.stop="toggleFavorite(item)"
                        title="收藏"
                    >
                      <StarIcon v-if="!item.isFavorite" class="w-3.5 h-3.5" />
                      <StarIconSolid v-else class="w-3.5 h-3.5 text-yellow-500" />
                    </button>
                    </div>
                  </div>
                </div>
                
                <!-- Empty state -->
                <div v-if="filteredHistory.length === 0" class="flex flex-col items-center justify-center py-8 px-3">
                  <div class="w-12 h-12 bg-gray-100 rounded-full flex items-center justify-center mb-3">
                    <MagnifyingGlassIcon class="w-6 h-6 text-gray-400" />
                  </div>
                  <p class="text-gray-500 text-xs text-center">
                    {{ searchQuery ? 'No items match your search' : currentTabInfo.emptyMessage }}
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
            </div>

            <div v-show="false" class="h-full flex flex-col min-h-0">
              <!-- Text List -->
              <div class="flex-1 overflow-y-auto min-h-0" @scroll="handleScroll">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  :data-item-id="item.id"
                  :title="item.note || ''"
                  class="group px-3 py-2 border-b border-gray-50 hover:bg-blue-50 cursor-pointer transition-all duration-200"
                  :class="{ 
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id && selectedItem?.id !== undefined,
                    'hover:bg-gray-50': selectedItem?.id !== item.id || selectedItem?.id === undefined
                  }"
                  @click="selectedItem = item"
                  @dblclick="handleDoubleClick(item)"
                  @contextmenu="showItemContextMenu($event, item)"
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
                              class="w-1.5 h-1.5 rounded-full bg-green-400"
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
                            <!-- 备注指示器 -->
                            <div 
                              v-if="item.note"
                              class="w-1.5 h-1.5 bg-blue-400 rounded-full"
                              :title="item.note"
                            ></div>
                          </div>
                        <p class="text-xs text-gray-900 line-clamp-2 leading-snug">
                          {{ item.type === 'text' ? item.content : 'Text content' }}
                        </p>
                      </div>
                    </div>
                    <div class="flex items-center space-x-1">
                      <!-- 删除按钮 -->
                      <button
                        class="flex-shrink-0 p-0.5 text-gray-400 hover:text-red-500 transition-colors duration-200 opacity-0 group-hover:opacity-100"
                        @click.stop="deleteItem(item)"
                        title="删除"
                      >
                        <TrashIcon class="w-3.5 h-3.5" />
                      </button>
                      <!-- 收藏按钮 -->
                      <button
                        class="flex-shrink-0 p-0.5 text-gray-400 hover:text-yellow-500 transition-colors duration-200 opacity-0 group-hover:opacity-100"
                        :class="{ 'opacity-100': item.isFavorite }"
                        @click.stop="toggleFavorite(item)"
                        title="收藏"
                      >
                        <StarIcon v-if="!item.isFavorite" class="w-3.5 h-3.5" />
                        <StarIconSolid v-else class="w-3.5 h-3.5 text-yellow-500" />
                      </button>
                  </div>
                </div>
              </div>

                <!-- Empty state for text -->
                <div v-if="filteredHistory.length === 0" class="flex flex-col items-center justify-center py-8 px-3">
                  <div class="w-12 h-12 bg-gray-100 rounded-full flex items-center justify-center mb-3">
                    <svg class="w-6 h-6 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"></path>
                    </svg>
                  </div>
                  <p class="text-gray-500 text-xs text-center">
                    {{ searchQuery ? 'No text matches your search' : 'No text content yet' }}
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
            </div>

            <div v-show="false" class="h-full flex flex-col min-h-0">
              <!-- Images List -->
              <div class="flex-1 overflow-y-auto min-h-0" @scroll="handleScroll">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  :data-item-id="item.id"
                  :title="item.note || ''"
                  class="group px-3 py-2 border-b border-gray-50 hover:bg-blue-50 cursor-pointer transition-all duration-200"
                  :class="{ 
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id && selectedItem?.id !== undefined,
                    'hover:bg-gray-50': selectedItem?.id !== item.id || selectedItem?.id === undefined
                  }"
                  @click="selectedItem = item"
                  @dblclick="handleDoubleClick(item)"
                  @contextmenu="showItemContextMenu($event, item)"
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
                              class="w-1.5 h-1.5 rounded-full bg-purple-400"
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
                            <!-- 备注指示器 -->
                            <div 
                              v-if="item.note"
                              class="w-1.5 h-1.5 bg-blue-400 rounded-full"
                              :title="item.note"
                            ></div>
                          </div>
                      <div class="mt-1">
                        <img 
                          v-if="getThumbnailSync(item)"
                          :src="getThumbnailSync(item)"
                          alt="Image thumbnail"
                          class="w-16 h-12 object-cover rounded border"
                          loading="lazy"
                          @error="($event.target as HTMLImageElement).style.display = 'none'"
                        />
                        <div 
                          v-else
                          class="w-16 h-12 bg-gray-100 rounded border flex items-center justify-center"
                        >
                          <svg class="w-6 h-6 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"></path>
                          </svg>
                        </div>
                      </div>
                    </div>
                  </div>
                  <div class="flex items-center space-x-1">
                    <!-- 删除按钮 -->
                    <button
                      class="flex-shrink-0 p-0.5 text-gray-400 hover:text-red-500 transition-colors duration-200 opacity-0 group-hover:opacity-100"
                      @click.stop="deleteItem(item)"
                      title="删除"
                    >
                      <TrashIcon class="w-3.5 h-3.5" />
                    </button>
                    <!-- 收藏按钮 -->
                    <button
                      class="flex-shrink-0 p-0.5 text-gray-400 hover:text-yellow-500 transition-colors duration-200 opacity-0 group-hover:opacity-100"
                        :class="{ 'opacity-100': item.isFavorite }"
                        @click.stop="toggleFavorite(item)"
                        title="收藏"
                      >
                        <StarIcon v-if="!item.isFavorite" class="w-3.5 h-3.5" />
                        <StarIconSolid v-else class="w-3.5 h-3.5 text-yellow-500" />
                      </button>
                  </div>
                  </div>
                </div>
                
                <!-- Empty state for images -->
                <div v-if="filteredHistory.length === 0" class="flex flex-col items-center justify-center py-8 px-3">
                  <div class="w-12 h-12 bg-gray-100 rounded-full flex items-center justify-center mb-3">
                    <svg class="w-6 h-6 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"></path>
                    </svg>
                  </div>
                  <p class="text-gray-500 text-xs text-center">
                    {{ searchQuery ? 'No images match your search' : 'No images yet' }}
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
            </div>

            <div v-show="false" class="h-full flex flex-col min-h-0">
              <!-- Favorites List -->
              <div class="flex-1 overflow-y-auto min-h-0" @scroll="handleScroll">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  :data-item-id="item.id"
                  :title="item.note || ''"
                  class="group px-3 py-2 border-b border-gray-50 hover:bg-blue-50 cursor-pointer transition-all duration-200"
                  :class="{ 
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id && selectedItem?.id !== undefined,
                    'hover:bg-gray-50': selectedItem?.id !== item.id || selectedItem?.id === undefined
                  }"
                  @click="selectedItem = item"
                  @dblclick="handleDoubleClick(item)"
                  @contextmenu="showItemContextMenu($event, item)"
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
                            <!-- 备注指示器 -->
                            <div 
                              v-if="item.note"
                              class="w-1.5 h-1.5 bg-blue-400 rounded-full"
                              :title="item.note"
                            ></div>
                        </div>
                        <div v-if="item.type === 'text'" class="text-xs text-gray-900 line-clamp-2 leading-snug">
                          {{ item.content }}
                      </div>
                        <div v-else class="mt-1">
                          <img 
                            v-if="getThumbnailSync(item)"
                            :src="getThumbnailSync(item)"
                            alt="Image thumbnail"
                            class="w-16 h-12 object-cover rounded border"
                            loading="lazy"
                            @error="($event.target as HTMLImageElement).style.display = 'none'"
                          />
                          <div 
                            v-else
                            class="w-16 h-12 bg-gray-100 rounded border flex items-center justify-center"
                          >
                            <svg class="w-6 h-6 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"></path>
                            </svg>
                    </div>
                        </div>
                      </div>
                    </div>
                    <div class="flex items-center space-x-1">
                      <!-- 删除按钮 -->
                      <button
                        class="flex-shrink-0 p-0.5 text-gray-400 hover:text-red-500 transition-colors duration-200"
                        @click.stop="deleteItem(item)"
                        title="删除"
                      >
                        <TrashIcon class="w-3.5 h-3.5" />
                      </button>
                      <!-- 收藏按钮 -->
                    <button
                      class="flex-shrink-0 p-0.5 text-yellow-500 hover:text-gray-400 transition-colors duration-200"
                      @click.stop="toggleFavorite(item)"
                        title="取消收藏"
                    >
                      <StarIconSolid class="w-3.5 h-3.5" />
                    </button>
                    </div>
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
            </div>
            
            <!-- Group List -->
            <div v-show="false" class="h-full flex flex-col min-h-0">
              <div class="flex-1 overflow-y-auto min-h-0">
                <div v-if="selectedGroupId && groups.find(g => g.id === selectedGroupId)" class="px-3 py-2 bg-blue-50 border-b border-blue-100">
                  <div class="flex items-center space-x-2">
                    <div 
                      class="w-3 h-3 rounded-full"
                      :style="{ backgroundColor: groups.find(g => g.id === selectedGroupId)?.color || '#3B82F6' }"
                    ></div>
                    <span class="text-sm font-medium text-blue-700">
                      {{ groups.find(g => g.id === selectedGroupId)?.name || '未知分组' }}
                    </span>
                    <span class="text-xs text-blue-500">({{ clipboardHistory.length }} 个条目)</span>
                  </div>
                </div>
                
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  :data-item-id="item.id"
                  :title="item.note || ''"
                  class="group px-3 py-2 border-b border-gray-50 hover:bg-blue-50 cursor-pointer transition-all duration-200"
                  :class="{ 
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id && selectedItem?.id !== undefined,
                    'hover:bg-gray-50': selectedItem?.id !== item.id || selectedItem?.id === undefined
                  }"
                  @click="selectedItem = item"
                  @contextmenu="showItemContextMenu($event, item)"
                >
                  <div class="flex items-start justify-between">
                    <div class="flex items-start space-x-2 flex-1 min-w-0 mr-2">
                      <!-- 源应用图标 -->
                      <div class="flex-shrink-0 w-6 h-6 mt-0.5">
                        <img 
                          v-if="item.sourceAppIcon" 
                          :src="item.sourceAppIcon" 
                          :alt="item.sourceAppName"
                          class="w-full h-full object-contain"
                        />
                        <div v-else class="w-full h-full bg-gray-100 rounded flex items-center justify-center">
                          <svg class="w-3 h-3 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z"></path>
                          </svg>
                        </div>
                      </div>
                      
                      <div class="flex-1 min-w-0">
                        <div class="flex items-center justify-between mb-1">
                          <div class="flex items-center space-x-1">
                            <div 
                              class="w-2 h-2 rounded-full"
                              :class="item.type === 'text' ? 'bg-green-400' : 'bg-purple-400'"
                            ></div>
                            <span class="text-xs text-gray-400">{{ item.sourceAppName }}</span>
                            <span v-if="item.note" class="text-xs text-orange-500" title="有备注">📝</span>
                          </div>
                          <span class="text-xs text-gray-400 flex-shrink-0">{{ formatTime(item.timestamp) }}</span>
                        </div>
                        
                        <div class="content-preview">
                          <template v-if="item.type === 'text'">
                            <p class="text-sm text-gray-900 line-clamp-2 break-words">{{ item.content }}</p>
                          </template>
                          <template v-else>
                            <div class="flex items-center space-x-2">
                              <div class="w-12 h-12 border border-gray-200 rounded overflow-hidden bg-gray-50 flex-shrink-0">
                                <img 
                                  v-if="getThumbnailSync(item)"
                                  :src="getThumbnailSync(item)"
                                  alt="Thumbnail"
                                  class="w-full h-full object-cover"
                                />
                                <div v-else class="w-full h-full flex items-center justify-center">
                                  <svg class="w-4 h-4 text-gray-400" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 16l4.586-4.586a2 2 0 012.828 0L16 16m-2-2l1.586-1.586a2 2 0 012.828 0L20 14m-6-6h.01M6 20h12a2 2 0 002-2V6a2 2 0 00-2-2H6a2 2 0 00-2 2v12a2 2 0 002 2z"></path>
                                  </svg>
                                </div>
                              </div>
                              <p class="text-sm text-gray-500">图片</p>
                            </div>
                          </template>
                        </div>
                      </div>
                    </div>

                    <div class="flex items-center space-x-1 opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex-shrink-0">
                      <!-- 删除按钮 -->
                      <button
                        @click.stop="deleteItem(item)"
                        class="p-0.5 text-gray-400 hover:text-red-500 transition-colors duration-200"
                        title="删除"
                      >
                        <TrashIcon class="w-3.5 h-3.5" />
                      </button>
                      <!-- 收藏按钮 -->
                    <button
                      class="flex-shrink-0 p-0.5 text-gray-400 hover:text-yellow-500 transition-colors duration-200 opacity-0 group-hover:opacity-100"
                      :class="{ 'opacity-100': item.isFavorite }"
                      @click.stop="toggleFavorite(item)"
                        title="收藏"
                    >
                      <StarIcon v-if="!item.isFavorite" class="w-3.5 h-3.5" />
                      <StarIconSolid v-else class="w-3.5 h-3.5 text-yellow-500" />
                    </button>
                    </div>
                  </div>
                </div>

                <!-- Loading states -->
                <div v-if="isLoadingMore" class="p-4 text-center">
                  <div class="flex items-center justify-center space-x-2">
                    <div class="animate-spin rounded-full h-4 w-4 border-b-2 border-blue-600"></div>
                    <span class="text-xs text-gray-500">Loading more...</span>
                  </div>
                </div>
                
                <div v-if="clipboardHistory.length === 0 && !isLoadingMore" class="p-8 text-center text-gray-400">
                  <div class="w-16 h-16 bg-gray-100 rounded-full flex items-center justify-center mx-auto mb-3">
                    <svg class="w-8 h-8" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14-7l2 2-2 2m2-2h-6m6 7l2 2-2 2m2-2h-6"></path>
                    </svg>
                  </div>
                  <p class="text-sm">该分组暂无条目</p>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- Right Content -->
      <div class="flex-1 flex flex-col min-h-0 bg-white">
        <div class="px-4 py-3 border-b border-gray-200 flex-shrink-0" data-tauri-drag-region>
          <div class="flex items-center justify-between" data-tauri-drag-region>
            <div class="flex items-center space-x-2" data-tauri-drag-region>
              <div 
                v-if="selectedItem"
                class="w-2.5 h-2.5 rounded-full"
                :class="selectedItem.type === 'text' ? 'bg-green-400' : 'bg-purple-400'"
              ></div>
              <h2 class="text-base font-semibold text-gray-900">
                {{ selectedItem?.type === 'text' ? '文本内容' : selectedItem?.type === 'image' ? '图片预览' : '选择条目' }}
              </h2>
            </div>
            <div class="flex items-center space-x-2">
              <span class="text-xs text-gray-500" v-if="selectedItem" data-tauri-drag-region>
              {{ formatTime(selectedItem.timestamp) }}
            </span>
              <button 
                class="p-1.5 text-gray-500 hover:text-gray-700 hover:bg-gray-100 rounded-md transition-colors duration-200"
                @click="showGroupManager = !showGroupManager"
                title="分组管理"
              >
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14-7l2 2-2 2m2-2h-6m6 7l2 2-2 2m2-2h-6"></path>
                </svg>
              </button>
              <button 
                class="p-1.5 text-gray-500 hover:text-gray-700 hover:bg-gray-100 rounded-md transition-colors duration-200"
                @click="showSettings = !showSettings"
                title="设置"
              >
                <Cog6ToothIcon class="w-4 h-4" />
              </button>
            </div>
          </div>
        </div>
        
        <div class="flex-1 p-4 overflow-y-auto min-h-0">
          <div v-if="selectedItem" class="h-full">
            <div class="bg-gray-50 rounded-lg border border-gray-200 p-4 min-h-full preview-container">
              <template v-if="selectedItem.type === 'text'">
                <div class="prose prose-sm max-w-none preview-content">
                  <pre class="whitespace-pre-wrap break-words text-gray-900 font-mono text-xs leading-normal preview-content">{{ selectedItem.content }}</pre>
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
            <p class="text-base font-medium mb-2">暂无剪贴板条目</p>
            <p class="text-xs text-center max-w-sm">
              从剪贴板历史中选择任何条目以查看其内容。 
              双击或按Enter键粘贴它。
            </p>
          </div>
        </div>
      </div>
    </div>

    <!-- 分组管理模态框 -->
    <div 
      v-if="showGroupManager"
      class="fixed inset-0 bg-black bg-opacity-30 flex items-center justify-center z-50"
      @click="showGroupManager = false"
    >
      <div 
        class="bg-white rounded-lg shadow-xl w-[500px] max-w-[90vw] max-h-[80vh] overflow-hidden"
        @click.stop
      >
        <div class="flex items-center justify-between p-4 border-b border-gray-200">
          <h2 class="text-lg font-semibold text-gray-900">分组管理</h2>
          <button
            @click="showGroupManager = false"
            class="text-gray-500 hover:text-gray-700 transition-colors"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
            </svg>
          </button>
        </div>
        
        <div class="p-4 max-h-[60vh] overflow-y-auto">
          <!-- 添加分组按钮 -->
          <button
            @click="openGroupForm()"
            class="w-full mb-4 px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white text-sm rounded-lg transition-colors duration-200 flex items-center justify-center space-x-2"
          >
            <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 4v16m8-8H4"></path>
            </svg>
            <span>新建分组</span>
          </button>
          
          <!-- 分组列表 -->
          <div v-if="groups.length === 0" class="text-center py-8 text-gray-500">
            <svg class="w-12 h-12 mx-auto mb-3 text-gray-300" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14-7l2 2-2 2m2-2h-6m6 7l2 2-2 2m2-2h-6"></path>
            </svg>
            <p>暂无分组</p>
            <p class="text-sm">点击上方按钮创建第一个分组</p>
          </div>
          
          <div v-else class="space-y-3">
            <div
              v-for="group in groups"
              :key="group.id"
              class="flex items-center justify-between p-3 bg-gray-50 rounded-lg hover:bg-gray-100 transition-colors"
            >
              <div class="flex items-center space-x-3">
                <div 
                  class="w-4 h-4 rounded-full"
                  :style="{ backgroundColor: group.color }"
                ></div>
                <div>
                  <h3 class="text-sm font-medium text-gray-900">{{ group.name }}</h3>
                  <p class="text-sm text-gray-500">{{ group.item_count }} 个条目</p>
                </div>
              </div>
              <div class="flex items-center space-x-2">
                <button
                  @click="openGroupForm(group)"
                  class="p-1.5 text-gray-500 hover:text-blue-600 hover:bg-blue-50 rounded transition-colors"
                  title="编辑分组"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"></path>
                  </svg>
                </button>
                <button
                  @click="deleteGroup(group)"
                  class="p-1.5 text-gray-500 hover:text-red-600 hover:bg-red-50 rounded transition-colors"
                  title="删除分组"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path>
                  </svg>
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- 分组表单模态框 -->
    <div 
      v-if="showGroupForm"
      class="fixed inset-0 bg-black bg-opacity-30 flex items-center justify-center z-50"
      @click="closeGroupForm"
    >
      <div 
        class="bg-white rounded-lg shadow-xl w-[400px] max-w-[90vw]"
        @click.stop
      >
        <div class="flex items-center justify-between p-4 border-b border-gray-200">
          <h2 class="text-lg font-semibold text-gray-900">
            {{ editingGroup ? '编辑分组' : '新建分组' }}
          </h2>
          <button
            @click="closeGroupForm"
            class="text-gray-500 hover:text-gray-700 transition-colors"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
            </svg>
          </button>
        </div>
        
        <div class="p-4 space-y-4">
          <div>
            <label class="block text-sm font-medium text-gray-700 mb-2">分组名称</label>
            <input
              v-model="groupForm.name"
              type="text"
              placeholder="请输入分组名称"
              class="w-full px-3 py-1 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
              @keydown.enter="saveGroup"
            />
          </div>
          
          <div>
            <label class="block text-sm font-medium text-gray-700 mb-2">分组颜色</label>
            <div class="flex items-center space-x-3">
              <input
                v-model="groupForm.color"
                type="color"
                class="w-12 h-8 border border-gray-300 rounded cursor-pointer"
              />
              <div class="flex space-x-2">
                <button
                  v-for="color in ['#3B82F6', '#10B981', '#F59E0B', '#EF4444', '#8B5CF6', '#F97316']"
                  :key="color"
                  @click="groupForm.color = color"
                  class="w-6 h-6 rounded-full border-2 transition-all"
                  :style="{ backgroundColor: color }"
                  :class="groupForm.color === color ? 'border-gray-800 scale-110' : 'border-gray-300'"
                ></button>
              </div>
            </div>
          </div>
        </div>
        
        <div class="flex justify-end space-x-2 p-3 border-t border-gray-200">
          <button
            @click="closeGroupForm"
            class="px-3 py-1.5 text-sm text-gray-600 bg-gray-100 hover:bg-gray-200 rounded transition-colors"
          >
            取消
          </button>
          <button
            @click="saveGroup"
            class="px-3 py-1.5 text-sm text-white bg-blue-600 hover:bg-blue-700 rounded transition-colors"
          >
            {{ editingGroup ? '更新' : '创建' }}
          </button>
        </div>
      </div>
    </div>

    <!-- 分组选择器模态框 -->
    <div 
      v-if="showGroupSelector"
      class="fixed inset-0 bg-black bg-opacity-30 flex items-center justify-center z-50"
      @click="closeGroupSelector"
    >
      <div 
        class="bg-white rounded-lg shadow-xl w-[350px] max-w-[90vw]"
        @click.stop
      >
        <div class="flex items-center justify-between px-4 py-3 border-b border-gray-200">
          <h2 class="text-lg font-semibold text-gray-900">选择分组</h2>
          <button
            @click="closeGroupSelector"
            class="text-gray-500 hover:text-gray-700 transition-colors"
          >
            <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"></path>
            </svg>
          </button>
        </div>
        
        <div class="p-4 max-h-[60vh] overflow-y-auto">
          <!-- 分组列表 -->
          <div v-if="groups.length === 0" class="text-center py-6 text-gray-500">
            <p>暂无分组</p>
            <button
              @click="closeGroupSelector(); showGroupManager = true"
              class="mt-2 text-blue-600 hover:text-blue-700 text-sm"
            >
              去创建分组
            </button>
          </div>
          
          <div v-else class="space-y-2">
            <button
              v-for="group in groups"
              :key="group.id"
              @click="addItemToGroup(group.id)"
              class="w-full p-2 text-left bg-gray-50 hover:bg-gray-100 rounded-lg transition-colors flex items-center space-x-3"
            >
              <div 
                class="w-4 h-4 rounded-full"
                :style="{ backgroundColor: group.color }"
              ></div>
              <div class="flex items-center justify-between flex-1">
                <span class="text-sm font-medium text-gray-900">{{ group.name }}</span>
                <span class="text-sm text-gray-500">{{ group.item_count }} 个条目</span>
              </div>
            </button>
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
    
    <!-- 右键菜单 -->
    <div 
      v-if="showContextMenu"
      :style="{ 
        position: 'fixed', 
        left: contextMenuPosition.x + 'px', 
        top: contextMenuPosition.y + 'px',
        zIndex: 9999
      }"
      class="bg-white rounded shadow-md border border-gray-200 py-0 w-20 text-xs"
      @click.stop
    >
      <button
        @click="handleContextMenuAction('note')"
        class="w-full pl-1.5 pr-3 py-0.5 text-left text-xs text-gray-700 hover:bg-gray-100 transition-colors duration-100"
      >
        备注
      </button>
      <button
        @click="handleContextMenuAction('group')"
        class="w-full pl-1.5 pr-3 py-0.5 text-left text-xs text-gray-700 hover:bg-gray-100 transition-colors duration-100"
      >
        分组
      </button>
      <!-- 移除分组选项（仅在分组模式下显示） -->
      <button
        v-if="currentTabInfo.isGroupMode"
        @click="handleContextMenuAction('remove-group')"
        class="w-full pl-1.5 pr-3 py-0.5 text-left text-xs text-gray-700 hover:bg-gray-100 transition-colors duration-100"
      >
        移除分组
      </button>
      <button
        @click="handleContextMenuAction('copy')"
        class="w-full pl-1.5 pr-3 py-0.5 text-left text-xs text-gray-700 hover:bg-gray-100 transition-colors duration-100"
      >
        复制
      </button>
      <button
        @click="handleContextMenuAction('favorite')"
        class="w-full pl-1.5 pr-3 py-0.5 text-left text-xs text-gray-700 hover:bg-gray-100 transition-colors duration-100"
      >
        收藏
      </button>
      <button
        @click="handleContextMenuAction('delete')"
        class="w-full pl-1.5 pr-3 py-0.5 text-left text-xs transition-colors duration-100"
      >
        删除
      </button>
    </div>

    <!-- 备注编辑对话框 -->
    <div 
      v-if="showNoteDialog"
      class="fixed inset-0 bg-black bg-opacity-30 flex items-center justify-center z-50"
      @click="closeNoteDialog"
    >
      <div 
        class="bg-white rounded shadow-lg p-4 w-80 max-w-[90vw]"
        @click.stop
      >
        <h3 class="text-sm font-medium text-gray-900 mb-3">
          {{ editingNoteItem?.note ? '编辑备注' : '添加备注' }}
        </h3>
        <input
          v-model="noteText"
          type="text"
          placeholder="请输入备注内容..."
          class="w-full p-1 text-sm border border-gray-300 rounded focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent"
          @keydown.esc="closeNoteDialog"
          @keydown.enter="saveNote"
          ref="noteInputRef"
        />
        <div class="flex justify-end space-x-2 mt-3">
          <button
            @click="closeNoteDialog"
            class="px-3 py-1.5 text-xs text-gray-600 bg-gray-100 hover:bg-gray-200 rounded transition-colors duration-100"
          >
            取消
          </button>
          <button
            @click="saveNote"
            class="px-3 py-1.5 text-xs text-white bg-blue-600 hover:bg-blue-700 rounded transition-colors duration-100"
          >
            保存
          </button>
        </div>
      </div>
    </div>
    
    <!-- Toast notifications -->
    <Toast 
      :messages="toastMessages" 
      @remove="removeToast" 
    />
    
    <!-- 确认对话框 -->
    <ConfirmDialog
      v-model:show="showConfirmDialog"
      :title="confirmDialog.title"
      :message="confirmDialog.message"
      :confirm-text="confirmDialog.confirmText"
      :cancel-text="confirmDialog.cancelText"
      :type="confirmDialog.type"
      @confirm="handleConfirmDialogConfirm"
    />
  </div>
</template>

<style>
/* 窗口拖拽区域样式 */
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

/* 极简按钮样式 */
.clean-nav-button {
  transition: all 0.1s ease;
  font-weight: 400;
  border: none;
}

.clean-nav-button:hover {
  transition: all 0.1s ease;
}

/* 响应式优化 */
@media (max-width: 640px) {
  .clean-nav-button {
    min-width: 45px;
    padding: 0.25rem 0.5rem;
    font-size: 0.75rem;
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