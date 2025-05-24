<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch, nextTick } from 'vue'
import { MagnifyingGlassIcon, StarIcon, Cog6ToothIcon } from '@heroicons/vue/24/outline'
import { StarIcon as StarIconSolid } from '@heroicons/vue/24/solid'
import { Tab, TabList, TabGroup, TabPanels, TabPanel } from '@headlessui/vue'
import Settings from './components/Settings.vue'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
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
  }

  if (newIndex !== currentIndex) {
    selectedItem.value = filteredHistory.value[newIndex]
  }
}

const handleTabChange = (index: number) => {
  console.log('Tab changed to:', index)
  selectedTabIndex.value = index
  // 重置搜索和选中状态
  searchQuery.value = ''
  selectedItem.value = null
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
  <div class="h-screen flex flex-col">
    <!-- Header -->
    <header class="bg-white border-b border-gray-200 p-4 flex-shrink-0">
      <div class="flex items-center justify-between">
        <h1 class="text-xl font-semibold">Clipboard Manager</h1>
        <div class="flex items-center space-x-4">
          <button class="btn btn-secondary" @click="openDevTools">
            Dev Tools
          </button>
          <button class="btn btn-secondary" @click="showSettings = !showSettings">
            <Cog6ToothIcon class="w-5 h-5" />
          </button>
        </div>
      </div>
    </header>

    <!-- Main Content -->
    <div class="flex-1 flex min-h-0">
      <!-- Left Sidebar -->
      <div class="w-1/3 border-r border-gray-200 flex flex-col min-h-0">
        <!-- Tabs -->
        <TabGroup v-model="selectedTabIndex" as="div" class="flex flex-col h-full" @change="handleTabChange">
          <div class="border-b border-gray-200 flex-shrink-0">
            <TabList class="flex">
              <!-- All 标签页 -->
              <Tab v-slot="{ selected }" as="template">
                <button
                  class="px-4 py-2.5 text-sm font-medium border-b-2 -mb-px"
                  :class="[
                    selected
                      ? 'text-primary-600 border-primary-600'
                      : 'text-gray-500 border-transparent hover:text-gray-700 hover:border-gray-300'
                  ]"
                >
                  All
                </button>
              </Tab>
              <!-- Favorites 标签页 -->
              <Tab v-slot="{ selected }" as="template">
                <button
                  class="px-4 py-2.5 text-sm font-medium border-b-2 -mb-px"
                  :class="[
                    selected
                      ? 'text-primary-600 border-primary-600'
                      : 'text-gray-500 border-transparent hover:text-gray-700 hover:border-gray-300'
                  ]"
                >
                  Favorites
                </button>
              </Tab>
            </TabList>
          </div>

          <TabPanels class="flex-1 min-h-0">
            <TabPanel class="h-full flex flex-col min-h-0">
              <!-- Search -->
              <div class="p-4 border-b border-gray-200 flex-shrink-0">
                <div class="relative">
                  <input
                    v-model="searchQuery"
                    type="text"
                    placeholder="Search clipboard history..."
                    class="w-full pl-10 pr-4 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                  <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                    <MagnifyingGlassIcon class="h-5 w-5 text-gray-400" />
                  </div>
                </div>
              </div>

              <!-- History List -->
              <div class="flex-1 overflow-y-auto min-h-0">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  class="p-4 border-b border-gray-200 hover:bg-gray-50 cursor-pointer"
                  :class="{ 'bg-blue-50': selectedItem?.id === item.id }"
                  @click="selectedItem = item"
                >
                  <div class="flex items-center justify-between">
                    <div class="flex-1 min-w-0">
                      <p class="text-sm truncate">
                        {{ item.type === 'text' ? item.content : 'Image' }}
                      </p>
                      <p class="text-xs text-gray-500">
                        {{ new Date(item.timestamp).toLocaleString() }}
                      </p>
                    </div>
                    <button
                      class="ml-2 text-gray-400 hover:text-yellow-500"
                      @click.stop="toggleFavorite(item)"
                    >
                      <StarIcon v-if="!item.isFavorite" class="w-5 h-5" />
                      <StarIconSolid v-else class="w-5 h-5 text-yellow-500" />
                    </button>
                  </div>
                </div>
              </div>
            </TabPanel>

            <TabPanel class="h-full flex flex-col min-h-0">
              <!-- Search -->
              <div class="p-4 border-b border-gray-200 flex-shrink-0">
                <div class="relative">
                  <input
                    v-model="searchQuery"
                    type="text"
                    placeholder="Search favorites..."
                    class="w-full pl-10 pr-4 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                  <div class="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                    <MagnifyingGlassIcon class="h-5 w-5 text-gray-400" />
                  </div>
                </div>
              </div>

              <!-- Favorites List -->
              <div class="flex-1 overflow-y-auto min-h-0">
                <div
                  v-for="item in filteredHistory"
                  :key="item.id"
                  class="p-4 border-b border-gray-200 hover:bg-gray-50 cursor-pointer"
                  :class="{ 'bg-blue-50': selectedItem?.id === item.id }"
                  @click="selectedItem = item"
                >
                  <div class="flex items-center justify-between">
                    <div class="flex-1 min-w-0">
                      <p class="text-sm truncate">
                        {{ item.type === 'text' ? item.content : 'Image' }}
                      </p>
                      <p class="text-xs text-gray-500">
                        {{ new Date(item.timestamp).toLocaleString() }}
                      </p>
                    </div>
                    <button
                      class="ml-2 text-yellow-500 hover:text-gray-400"
                      @click.stop="toggleFavorite(item)"
                    >
                      <StarIconSolid class="w-5 h-5" />
                    </button>
                  </div>
                </div>
              </div>
            </TabPanel>
          </TabPanels>
        </TabGroup>
      </div>

      <!-- Right Content -->
      <div class="flex-1 flex flex-col min-h-0">
        <div class="p-4 border-b border-gray-200 flex-shrink-0">
          <div class="flex items-center justify-between">
            <h2 class="text-lg font-semibold">
              {{ selectedItem?.type === 'text' ? 'Text Content' : 'Image Preview' }}
            </h2>
            <span class="text-sm text-gray-500" v-if="selectedItem">
              {{ new Date(selectedItem.timestamp).toLocaleString() }}
            </span>
          </div>
        </div>
        
        <div class="flex-1 p-4 overflow-y-auto min-h-0">
          <div v-if="selectedItem" class="h-full">
            <div class="bg-white rounded-lg border border-gray-200 p-4">
              <template v-if="selectedItem.type === 'text'">
                <p class="whitespace-pre-wrap break-words">{{ selectedItem.content }}</p>
              </template>
              <template v-else>
                <img
                  :src="selectedItem.content"
                  alt="Clipboard image"
                  class="max-w-full h-auto rounded-lg"
                />
              </template>
            </div>
          </div>
          <div v-else class="h-full flex items-center justify-center text-gray-500">
            Select an item to preview
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
  width: 8px;
  height: 8px;
}

::-webkit-scrollbar-track {
  background: transparent;
}

::-webkit-scrollbar-thumb {
  background: #d1d5db;
  border-radius: 4px;
}

::-webkit-scrollbar-thumb:hover {
  background: #9ca3af;
}

/* 确保图标大小正确 */
.heroicon {
  width: 1.5rem;
  height: 1.5rem;
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