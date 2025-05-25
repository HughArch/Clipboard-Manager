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
const lastContent = ref<{ content: string; timestamp: number } | null>(null)
const DEBOUNCE_TIME = 3000 // 3秒内的重复内容不记录
let db: Awaited<ReturnType<any>> | null = null

// 搜索框引用
const searchInputRef = ref<HTMLInputElement | null>(null)

// 自动聚焦搜索框
const focusSearchInput = async () => {
  await nextTick()
  if (searchInputRef.value) {
    searchInputRef.value.focus()
    console.log('Search input focused')
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

// 检查是否是重复内容
const isDuplicateContent = (content: string): boolean => {
  if (!lastContent.value) return false
  
  const now = Date.now()
  const timeDiff = now - lastContent.value.timestamp
  
  // 如果是相同内容且在防抖时间内
  if (lastContent.value.content === content && timeDiff < DEBOUNCE_TIME) {
    console.log('Duplicate content detected within', DEBOUNCE_TIME, 'ms, skipping...')
    return true
  }
  
  // 更新最后复制的内容和时间
  lastContent.value = { content, timestamp: now }
  return false
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
    await invoke('paste_to_clipboard', {
      content: item.content,
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
      `SELECT id, content, type, timestamp, is_favorite, image_path 
       FROM clipboard_history 
       ORDER BY id DESC`
    )
    clipboardHistory.value = rows.map((row: any) => ({
      id: row.id,
      content: row.content,
      type: row.type,
      timestamp: row.timestamp,
      isFavorite: row.is_favorite === 1,
      imagePath: row.image_path ?? null
    }))

    listen<string>('clipboard-text', async (event) => {
      // 检查是否是短时间内的重复内容
      if (isDuplicateContent(event.payload)) return

      const item = {
        content: event.payload,
        type: 'text',
        timestamp: new Date().toISOString(),
        isFavorite: false,
        imagePath: null
      }
      // 插入新记录到数据库
      await db.execute(
        `INSERT INTO clipboard_history (content, type, timestamp, is_favorite, image_path) 
         VALUES (?, ?, ?, ?, ?)`,
        [item.content, item.type, item.timestamp, 0, item.imagePath]
      )
      const rows = await db.select(`SELECT last_insert_rowid() as id`)
      const id = rows[0]?.id || Date.now()
      clipboardHistory.value.unshift(Object.assign({ id }, item))
    })

    listen<string>('clipboard-image', async (event) => {
      // 检查是否是短时间内的重复内容
      if (isDuplicateContent(event.payload)) return

      const item = {
        content: event.payload,
        type: 'image',
        timestamp: new Date().toISOString(),
        isFavorite: false,
        imagePath: null
      }
      // 插入新记录到数据库
      await db.execute(
        `INSERT INTO clipboard_history (content, type, timestamp, is_favorite, image_path) 
         VALUES (?, ?, ?, ?, ?)`,
        [item.content, item.type, item.timestamp, 0, item.imagePath]
      )
      const rows = await db.select(`SELECT last_insert_rowid() as id`)
      const id = rows[0]?.id || Date.now()
      clipboardHistory.value.unshift(Object.assign({ id }, item))
    })

    window.addEventListener('keydown', handleKeyDown)
    
    // 组件挂载后自动聚焦搜索框
    await focusSearchInput()
  } catch (error) {
    console.error('Database error:', error)
  }
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeyDown)
})

// 监听标签页变化
watch(selectedTabIndex, () => {
  // 切换标签页时重置搜索
  searchQuery.value = ''
  // 重置选中项
  selectedItem.value = null
})
</script>

<template>
  <div class="h-screen flex flex-col bg-gray-50">
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
          <button 
            class="px-3 py-2 text-sm font-medium text-gray-600 hover:text-gray-900 hover:bg-gray-100 rounded-lg transition-colors duration-200"
            @click="openDevTools"
          >
            Dev Tools
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
      <div class="w-96 bg-white border-r border-gray-200 flex flex-col min-h-0 shadow-sm">
        <!-- Tabs -->
        <TabGroup v-model="selectedTabIndex" as="div" class="flex flex-col h-full" @change="handleTabChange">
          <div class="border-b border-gray-200 flex-shrink-0 bg-gray-50">
            <TabList class="flex">
              <!-- All 标签页 -->
              <Tab v-slot="{ selected }" as="template">
                <button
                  class="flex-1 px-6 py-3 text-sm font-medium border-b-2 -mb-px transition-all duration-200"
                  :class="[
                    selected
                      ? 'text-blue-600 border-blue-600 bg-white'
                      : 'text-gray-500 border-transparent hover:text-gray-700 hover:border-gray-300 bg-gray-50'
                  ]"
                >
                  <span class="flex items-center space-x-2">
                    <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 11H5m14 0a2 2 0 012 2v6a2 2 0 01-2 2H5a2 2 0 01-2-2v-6a2 2 0 012-2m14 0V9a2 2 0 00-2-2M5 11V9a2 2 0 012-2m0 0V5a2 2 0 012-2h6a2 2 0 012 2v2M7 7h10"></path>
                    </svg>
                    <span>All</span>
                  </span>
                </button>
              </Tab>
              <!-- Favorites 标签页 -->
              <Tab v-slot="{ selected }" as="template">
                <button
                  class="flex-1 px-6 py-3 text-sm font-medium border-b-2 -mb-px transition-all duration-200"
                  :class="[
                    selected
                      ? 'text-blue-600 border-blue-600 bg-white'
                      : 'text-gray-500 border-transparent hover:text-gray-700 hover:border-gray-300 bg-gray-50'
                  ]"
                >
                  <span class="flex items-center space-x-2">
                    <StarIcon class="w-4 h-4" />
                    <span>Favorites</span>
                  </span>
                </button>
              </Tab>
            </TabList>
          </div>

          <TabPanels class="flex-1 min-h-0">
            <TabPanel class="h-full flex flex-col min-h-0">
              <!-- Search -->
              <div class="p-4 border-b border-gray-100 flex-shrink-0">
                <div class="relative">
                  <input
                    v-model="searchQuery"
                    type="text"
                    placeholder="Search clipboard history..."
                    class="w-full pl-10 pr-4 py-2.5 border border-gray-200 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all duration-200 text-sm"
                    ref="searchInputRef"
                  />
                  <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                    <MagnifyingGlassIcon class="h-4 w-4 text-gray-400" />
                  </div>
                </div>
              </div>

              <!-- History List -->
              <div class="flex-1 overflow-y-auto min-h-0">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  :data-item-id="item.id"
                  class="group px-4 py-3 border-b border-gray-50 hover:bg-blue-50 cursor-pointer transition-all duration-200"
                  :class="{ 
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id,
                    'hover:bg-gray-50': selectedItem?.id !== item.id
                  }"
                  @click="selectedItem = item"
                  @dblclick="handleDoubleClick(item)"
                >
                  <div class="flex items-start justify-between">
                    <div class="flex-1 min-w-0 mr-3">
                      <div class="flex items-center space-x-2 mb-1">
                        <div class="flex items-center space-x-1">
                          <div 
                            class="w-2 h-2 rounded-full"
                            :class="item.type === 'text' ? 'bg-green-400' : 'bg-purple-400'"
                          ></div>
                          <span class="text-xs font-medium text-gray-500 uppercase tracking-wide">
                            {{ item.type }}
                          </span>
                        </div>
                      </div>
                      <p class="text-sm text-gray-900 line-clamp-2 leading-relaxed">
                        {{ item.type === 'text' ? item.content : 'Image content' }}
                      </p>
                      <p class="text-xs text-gray-500 mt-1">
                        {{ new Date(item.timestamp).toLocaleString() }}
                      </p>
                    </div>
                    <button
                      class="flex-shrink-0 p-1 text-gray-400 hover:text-yellow-500 transition-colors duration-200 opacity-0 group-hover:opacity-100"
                      :class="{ 'opacity-100': item.isFavorite }"
                      @click.stop="toggleFavorite(item)"
                    >
                      <StarIcon v-if="!item.isFavorite" class="w-4 h-4" />
                      <StarIconSolid v-else class="w-4 h-4 text-yellow-500" />
                    </button>
                  </div>
                </div>
                
                <!-- Empty state -->
                <div v-if="filteredHistory.length === 0" class="flex flex-col items-center justify-center py-12 px-4">
                  <div class="w-16 h-16 bg-gray-100 rounded-full flex items-center justify-center mb-4">
                    <MagnifyingGlassIcon class="w-8 h-8 text-gray-400" />
                  </div>
                  <p class="text-gray-500 text-sm text-center">
                    {{ searchQuery ? 'No items match your search' : 'No clipboard history yet' }}
                  </p>
                </div>
              </div>
            </TabPanel>

            <TabPanel class="h-full flex flex-col min-h-0">
              <!-- Search -->
              <div class="p-4 border-b border-gray-100 flex-shrink-0">
                <div class="relative">
                  <input
                    v-model="searchQuery"
                    type="text"
                    placeholder="Search favorites..."
                    class="w-full pl-10 pr-4 py-2.5 border border-gray-200 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all duration-200 text-sm"
                    ref="searchInputRef"
                  />
                  <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                    <MagnifyingGlassIcon class="h-4 w-4 text-gray-400" />
                  </div>
                </div>
              </div>

              <!-- Favorites List -->
              <div class="flex-1 overflow-y-auto min-h-0">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  :data-item-id="item.id"
                  class="group px-4 py-3 border-b border-gray-50 hover:bg-blue-50 cursor-pointer transition-all duration-200"
                  :class="{ 
                    'bg-blue-100 border-blue-200': selectedItem?.id === item.id,
                    'hover:bg-gray-50': selectedItem?.id !== item.id
                  }"
                  @click="selectedItem = item"
                  @dblclick="handleDoubleClick(item)"
                >
                  <div class="flex items-start justify-between">
                    <div class="flex-1 min-w-0 mr-3">
                      <div class="flex items-center space-x-2 mb-1">
                        <div class="flex items-center space-x-1">
                          <div 
                            class="w-2 h-2 rounded-full"
                            :class="item.type === 'text' ? 'bg-green-400' : 'bg-purple-400'"
                          ></div>
                          <span class="text-xs font-medium text-gray-500 uppercase tracking-wide">
                            {{ item.type }}
                          </span>
                        </div>
                      </div>
                      <p class="text-sm text-gray-900 line-clamp-2 leading-relaxed">
                        {{ item.type === 'text' ? item.content : 'Image content' }}
                      </p>
                      <p class="text-xs text-gray-500 mt-1">
                        {{ new Date(item.timestamp).toLocaleString() }}
                      </p>
                    </div>
                    <button
                      class="flex-shrink-0 p-1 text-yellow-500 hover:text-gray-400 transition-colors duration-200"
                      @click.stop="toggleFavorite(item)"
                    >
                      <StarIconSolid class="w-4 h-4" />
                    </button>
                  </div>
                </div>
                
                <!-- Empty state for favorites -->
                <div v-if="filteredHistory.length === 0" class="flex flex-col items-center justify-center py-12 px-4">
                  <div class="w-16 h-16 bg-gray-100 rounded-full flex items-center justify-center mb-4">
                    <StarIcon class="w-8 h-8 text-gray-400" />
                  </div>
                  <p class="text-gray-500 text-sm text-center">
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
        <div class="px-6 py-4 border-b border-gray-200 flex-shrink-0">
          <div class="flex items-center justify-between">
            <div class="flex items-center space-x-3">
              <div 
                v-if="selectedItem"
                class="w-3 h-3 rounded-full"
                :class="selectedItem.type === 'text' ? 'bg-green-400' : 'bg-purple-400'"
              ></div>
              <h2 class="text-lg font-semibold text-gray-900">
                {{ selectedItem?.type === 'text' ? 'Text Content' : selectedItem?.type === 'image' ? 'Image Preview' : 'Select an Item' }}
              </h2>
            </div>
            <span class="text-sm text-gray-500" v-if="selectedItem">
              {{ new Date(selectedItem.timestamp).toLocaleString() }}
            </span>
          </div>
        </div>
        
        <div class="flex-1 p-6 overflow-y-auto min-h-0">
          <div v-if="selectedItem" class="h-full">
            <div class="bg-gray-50 rounded-xl border border-gray-200 p-6 min-h-full">
              <template v-if="selectedItem.type === 'text'">
                <div class="prose prose-sm max-w-none">
                  <pre class="whitespace-pre-wrap break-words text-gray-900 font-mono text-sm leading-relaxed">{{ selectedItem.content }}</pre>
                </div>
              </template>
              <template v-else>
                <div class="flex items-center justify-center">
                  <img
                    :src="selectedItem.content"
                    alt="Clipboard image"
                    class="max-w-full max-h-full object-contain rounded-lg shadow-lg"
                  />
                </div>
              </template>
            </div>
          </div>
          <div v-else class="h-full flex flex-col items-center justify-center text-gray-400">
            <div class="w-20 h-20 bg-gray-100 rounded-full flex items-center justify-center mb-4">
              <svg class="w-10 h-10" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M15 15l-2 5L9 9l11 4-5 2zm0 0l5 5M7.188 2.239l.777 2.897M5.136 7.965l-2.898-.777M13.95 4.05l-2.122 2.122m-5.657 5.656l-2.12 2.122"></path>
              </svg>
            </div>
            <p class="text-lg font-medium mb-2">Select an item to preview</p>
            <p class="text-sm text-center max-w-sm">
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

/* 响应式字体 */
@media (max-width: 768px) {
  .text-xl {
    font-size: 1.125rem;
  }
  
  .w-96 {
    width: 20rem;
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