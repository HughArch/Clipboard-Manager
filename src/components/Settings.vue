<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

interface AppSettings {
  max_history_items: number
  max_history_time: number
  hotkey: string
  auto_start: boolean
}

defineProps<{
  show: boolean
}>()

const emit = defineEmits<{
  (e: 'update:show', value: boolean): void
  (e: 'save-settings', settings: AppSettings): void
}>()

const settings = ref<AppSettings>({
  max_history_items: 100,
  max_history_time: 30,
  hotkey: 'Ctrl+Shift+V',
  auto_start: false
})

// 加载设置
onMounted(async () => {
  try {
    const savedSettings = await invoke<AppSettings>('load_settings')
    settings.value = savedSettings
    
    // 获取当前自启动状态，确保界面显示与实际状态一致
    try {
      const autoStartStatus = await invoke<boolean>('get_auto_start_status')
      settings.value.auto_start = autoStartStatus
    } catch (error) {
      console.warn('Failed to get auto-start status:', error)
    }
  } catch (error) {
    console.error('Failed to load settings:', error)
  }
})

const handleSubmit = async () => {
  try {
    // 保存设置
    await invoke('save_settings', { settings: settings.value })
    // 更新快捷键
    await invoke('register_shortcut', { shortcut: settings.value.hotkey })
    // 更新自启动设置
    await invoke('set_auto_start', { enable: settings.value.auto_start })
    emit('save-settings', settings.value)
    emit('update:show', false)
  } catch (error) {
    console.error('Failed to save settings:', error)
    // 这里可以添加错误提示
  }
}
</script>

<template>
  <div v-if="show" class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50 p-4">
    <div class="bg-white rounded-xl shadow-2xl w-full max-w-lg transform transition-all duration-300">
      <!-- Header -->
      <div class="px-6 py-4 border-b border-gray-200">
        <div class="flex items-center space-x-3">
          <div class="w-8 h-8 bg-gradient-to-br from-blue-500 to-purple-600 rounded-lg flex items-center justify-center">
            <svg class="w-5 h-5 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"></path>
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"></path>
            </svg>
          </div>
          <h2 class="text-xl font-semibold text-gray-900">Settings</h2>
        </div>
      </div>
      
      <!-- Content -->
      <div class="p-6">
        <form @submit.prevent="handleSubmit" class="space-y-6">
          <!-- 最大历史记录数 -->
          <div class="space-y-2">
            <label class="block text-sm font-medium text-gray-700">
              Max History Items
            </label>
            <input
              v-model.number="settings.max_history_items"
              type="number"
              min="1"
              class="w-full border border-gray-200 rounded-lg px-4 py-2.5 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all duration-200 text-sm"
            />
            <p class="text-xs text-gray-500">Maximum number of items to keep in history</p>
          </div>

          <!-- 最大保存时间（天） -->
          <div class="space-y-2">
            <label class="block text-sm font-medium text-gray-700">
              Max History Time (days)
            </label>
            <input
              v-model.number="settings.max_history_time"
              type="number"
              min="1"
              class="w-full border border-gray-200 rounded-lg px-4 py-2.5 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all duration-200 text-sm"
            />
            <p class="text-xs text-gray-500">Automatically delete items older than this many days</p>
          </div>

          <!-- 快捷键 -->
          <div class="space-y-2">
            <label class="block text-sm font-medium text-gray-700">
              Global Hotkey
            </label>
            <input
              v-model="settings.hotkey"
              type="text"
              placeholder="e.g. Ctrl+Shift+V"
              class="w-full border border-gray-200 rounded-lg px-4 py-2.5 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all duration-200 text-sm"
            />
            <p class="text-xs text-gray-500">
              Format: Ctrl/Alt/Shift/Meta + Letter/Number (e.g. Ctrl+Shift+V)
            </p>
          </div>

          <!-- 开机自启动 -->
          <div class="space-y-2">
            <label class="flex items-center space-x-3 p-3 bg-gray-50 rounded-lg hover:bg-gray-100 transition-colors duration-200 cursor-pointer">
              <input
                v-model="settings.auto_start"
                type="checkbox"
                class="w-4 h-4 text-blue-600 bg-gray-100 border-gray-300 rounded focus:ring-blue-500 focus:ring-2"
              />
              <div class="flex-1">
                <span class="text-sm font-medium text-gray-700">Start with system</span>
                <p class="text-xs text-gray-500">Automatically start the application when your computer boots</p>
              </div>
            </label>
          </div>

          <!-- 按钮组 -->
          <div class="flex justify-end space-x-3 pt-4 border-t border-gray-200">
            <button
              type="button"
              class="px-4 py-2.5 text-sm font-medium text-gray-700 hover:text-gray-900 hover:bg-gray-100 rounded-lg transition-all duration-200"
              @click="$emit('update:show', false)"
            >
              Cancel
            </button>
            <button
              type="submit"
              class="px-6 py-2.5 text-sm font-medium text-white bg-gradient-to-r from-blue-500 to-purple-600 hover:from-blue-600 hover:to-purple-700 rounded-lg transition-all duration-200 shadow-md hover:shadow-lg transform hover:-translate-y-0.5"
            >
              Save Settings
            </button>
          </div>
        </form>
      </div>
    </div>
  </div>
</template> 