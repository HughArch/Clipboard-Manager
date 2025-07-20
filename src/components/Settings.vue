<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { logger } from '../composables/useLogger'
import ConfirmDialog from './ConfirmDialog.vue'

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
  (e: 'show-toast', toast: { type: 'success' | 'error' | 'warning' | 'info', title: string, message?: string, duration?: number }): void
}>()

const settings = ref<AppSettings>({
  max_history_items: 100,
  max_history_time: 30,
  hotkey: 'Ctrl+Shift+V',
  auto_start: false
})

// 键盘录制状态
const isRecording = ref(false)
const recordingKeys = ref<Set<string>>(new Set())
const hotkeyInputRef = ref<HTMLInputElement | null>(null)

// 确认弹窗状态
const showConfirmDialog = ref(false)

// 打开日志文件夹
const openLogFolder = async () => {
  try {
    await invoke('open_log_folder')
    logger.info('打开日志文件夹成功')
    emit('show-toast', { type: 'success', title: 'Folder Opened', message: 'Log folder opened successfully' })
  } catch (error) {
    logger.error('打开日志文件夹失败', { error: String(error) })
    emit('show-toast', { type: 'error', title: 'Open Failed', message: 'Failed to open log folder' })
  }
}

// 删除所有日志
const deleteAllLogs = async () => {
  showConfirmDialog.value = true
}

// 确认删除日志
const confirmDeleteLogs = async () => {
  try {
    await invoke('delete_all_logs')
    logger.info('删除所有日志成功')
    emit('show-toast', { type: 'success', title: 'Logs Deleted', message: 'All log files have been deleted successfully' })
  } catch (error) {
    logger.error('删除所有日志失败', { error: String(error) })
    emit('show-toast', { type: 'error', title: 'Delete Failed', message: 'Failed to delete log files' })
  }
}

// 键盘录制功能
const startRecording = () => {
  isRecording.value = true
  recordingKeys.value.clear()
  
  // 聚焦到输入框
  if (hotkeyInputRef.value) {
    hotkeyInputRef.value.focus()
  }
}

const stopRecording = () => {
  isRecording.value = false
  
  // 如果录制了按键，生成快捷键字符串
  if (recordingKeys.value.size > 0) {
    const keys = Array.from(recordingKeys.value)
    const hotkeyString = generateHotkeyString(keys)
    settings.value.hotkey = hotkeyString
  }
  
  recordingKeys.value.clear()
  
  // 失去焦点
  if (hotkeyInputRef.value) {
    hotkeyInputRef.value.blur()
  }
}

const generateHotkeyString = (keys: string[]): string => {
  const modifiers: string[] = []
  let mainKey = ''
  
  // 分离修饰键和主键
  keys.forEach(key => {
    switch (key.toLowerCase()) {
      case 'control':
      case 'ctrl':
        if (!modifiers.includes('Ctrl')) modifiers.push('Ctrl')
        break
      case 'alt':
        if (!modifiers.includes('Alt')) modifiers.push('Alt')
        break
      case 'shift':
        if (!modifiers.includes('Shift')) modifiers.push('Shift')
        break
      case 'meta':
      case 'cmd':
        if (!modifiers.includes('Meta')) modifiers.push('Meta')
        break
      default:
        // 主键，取最后一个非修饰键
        if (key.length === 1 || ['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12'].includes(key)) {
          mainKey = key.toUpperCase()
        }
        break
    }
  })
  
  // 如果没有主键，使用默认
  if (!mainKey && modifiers.length > 0) {
    mainKey = 'V'
  }
  
  // 如果没有修饰键但有主键，添加默认修饰键
  if (mainKey && modifiers.length === 0) {
    modifiers.push('Ctrl', 'Shift')
  }
  
  // 组合成快捷键字符串
  if (mainKey) {
    return [...modifiers, mainKey].join('+')
  }
  
  return 'Ctrl+Shift+V' // 默认值
}

const handleKeyDown = (event: KeyboardEvent) => {
  if (!isRecording.value) return
  
  event.preventDefault()
  event.stopPropagation()
  
  // 记录按下的键
  if (event.ctrlKey) recordingKeys.value.add('Ctrl')
  if (event.altKey) recordingKeys.value.add('Alt')
  if (event.shiftKey) recordingKeys.value.add('Shift')
  if (event.metaKey) recordingKeys.value.add('Meta')
  
  // 记录主键
  if (event.key && event.key.length === 1) {
    recordingKeys.value.add(event.key)
  } else if (event.key && event.key.startsWith('F') && event.key.length <= 3) {
    recordingKeys.value.add(event.key)
  }
  
  // 如果有修饰键+主键，自动停止录制
  const hasModifier = event.ctrlKey || event.altKey || event.shiftKey || event.metaKey
  const hasMainKey = event.key && (event.key.length === 1 || event.key.startsWith('F'))
  
  if (hasModifier && hasMainKey) {
    setTimeout(() => {
      stopRecording()
    }, 100) // 短暂延迟确保按键被记录
  }
}

const handleKeyUp = (event: KeyboardEvent) => {
  if (!isRecording.value) return
  
  event.preventDefault()
  event.stopPropagation()
}

// 获取当前录制的快捷键预览
const getRecordingPreview = (): string => {
  if (recordingKeys.value.size === 0) return ''
  const keys = Array.from(recordingKeys.value)
  return generateHotkeyString(keys)
}

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

// 解析快捷键冲突错误
const parseHotkeyError = (error: string): { title: string; message: string } => {
  if (error.includes('HotKey already registered')) {
    return {
      title: 'Hotkey Conflict',
      message: `"${settings.value.hotkey}" is already in use. Try a different combination.`
    }
  }
  
  if (error.includes('Invalid hotkey format')) {
    return {
      title: 'Invalid Format',
      message: 'Use format: Ctrl+Shift+V'
    }
  }
  
  return {
    title: 'Registration Failed',
    message: 'Please try a different hotkey'
  }
}

const handleSubmit = async () => {
  try {
    // 保存设置
    await invoke('save_settings', { settings: settings.value })
    
    // 更新快捷键
    try {
      await invoke('register_shortcut', { shortcut: settings.value.hotkey })
    } catch (hotkeyError) {
      const { title, message } = parseHotkeyError(String(hotkeyError))
      emit('show-toast', { type: 'error', title, message, duration: 8000 }) // 显示8秒，让用户有时间阅读
      return // 如果快捷键注册失败，不继续
    }
    
    // 更新自启动设置
    try {
      await invoke('set_auto_start', { enable: settings.value.auto_start })
    } catch (autoStartError) {
      // 自启动失败不影响其他设置，显示警告即可
      emit('show-toast', {
        type: 'warning',
        title: 'Auto-start Warning',
        message: 'Auto-start setup failed, but other settings saved.',
        duration: 6000
      })
    }
    
    // 所有操作成功
    emit('show-toast', { type: 'success', title: 'Settings Saved', message: 'All settings updated successfully!', duration: 3000 })
    emit('save-settings', settings.value)
    emit('update:show', false)
    
  } catch (error) {
    console.error('Failed to save settings:', error)
    emit('show-toast', {
      type: 'error',
      title: 'Save Failed',
      message: 'Settings could not be saved. Try again.',
      duration: 5000
    })
  }
}
</script>

<template>
  <div v-if="show" class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-[9998] p-3">
    <div class="bg-white rounded-xl shadow-2xl w-full max-w-md transform transition-all duration-300 max-h-[90vh] overflow-y-auto">
      <!-- Header -->
      <div class="px-4 py-3 border-b border-gray-200">
        <div class="flex items-center space-x-2">
          <div class="w-6 h-6 bg-gradient-to-br from-blue-500 to-purple-600 rounded-lg flex items-center justify-center">
            <svg class="w-4 h-4 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"></path>
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"></path>
            </svg>
          </div>
          <h2 class="text-lg font-semibold text-gray-900">Settings</h2>
        </div>
      </div>
      
      <!-- Content -->
      <div class="p-4">
        <form @submit.prevent="handleSubmit" class="space-y-4">
          <!-- General Settings Section -->
          <div class="space-y-3">
            <h3 class="text-sm font-medium text-gray-900 border-b border-gray-100 pb-1">General</h3>
            
            <!-- 最大历史记录数和时间 - 一行两列 -->
            <div class="grid grid-cols-2 gap-3">
              <div class="space-y-1">
                <label class="block text-xs font-medium text-gray-700">Max Items</label>
            <input
              v-model.number="settings.max_history_items"
              type="number"
              min="1"
                  class="w-full border border-gray-200 rounded-md px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent"
            />
          </div>
              <div class="space-y-1">
                <label class="block text-xs font-medium text-gray-700">Max Days</label>
            <input
              v-model.number="settings.max_history_time"
              type="number"
              min="1"
                  class="w-full border border-gray-200 rounded-md px-2 py-1.5 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent"
            />
              </div>
          </div>

          <!-- 快捷键 -->
            <div class="space-y-1">
              <label class="block text-xs font-medium text-gray-700">Global Hotkey</label>
            <div class="relative">
              <input
                ref="hotkeyInputRef"
                v-model="settings.hotkey"
                type="text"
                  :placeholder="isRecording ? 'Press keys...' : 'e.g. Ctrl+Shift+V'"
                :readonly="isRecording"
                  class="w-full border rounded-md px-2 py-1.5 pr-14 text-xs focus:outline-none focus:ring-1 focus:ring-blue-500 focus:border-transparent"
                :class="{
                  'border-blue-400 bg-blue-50': isRecording,
                  'border-gray-200': !isRecording
                }"
                @keydown="handleKeyDown"
                @keyup="handleKeyUp"
                @blur="isRecording && stopRecording()"
              />
              <button
                type="button"
                @click="isRecording ? stopRecording() : startRecording()"
                  class="absolute right-1 top-1/2 transform -translate-y-1/2 px-2 py-0.5 text-xs font-medium rounded transition-all duration-200"
                :class="{
                  'bg-red-500 text-white hover:bg-red-600': isRecording,
                  'bg-blue-500 text-white hover:bg-blue-600': !isRecording
                }"
              >
                {{ isRecording ? 'Stop' : 'Record' }}
              </button>
            </div>
              <p v-if="isRecording && getRecordingPreview()" class="text-xs text-blue-600">
                Preview: {{ getRecordingPreview() }}
              </p>
          </div>

          <!-- 开机自启动 -->
            <label class="flex items-center space-x-2 p-2 bg-gray-50 rounded-md hover:bg-gray-100 transition-colors duration-200 cursor-pointer">
              <input
                v-model="settings.auto_start"
                type="checkbox"
                class="w-3 h-3 text-blue-600 bg-gray-100 border-gray-300 rounded focus:ring-blue-500 focus:ring-1"
              />
              <span class="text-xs font-medium text-gray-700">Start with system</span>
            </label>
          </div>

          <!-- Log Management Section -->
          <div class="space-y-3">
            <h3 class="text-sm font-medium text-gray-900 border-b border-gray-100 pb-1">Log Management</h3>
            
            <div class="grid grid-cols-2 gap-2">
              <!-- Open Log Folder -->
              <button
                type="button"
                @click="openLogFolder"
                class="flex items-center justify-center space-x-1 p-2 bg-green-50 hover:bg-green-100 border border-green-200 rounded-md transition-colors duration-200"
              >
                <svg class="w-3 h-3 text-green-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-5l-2-2H5a2 2 0 00-2 2z"></path>
                </svg>
                <span class="text-xs font-medium text-green-700">Open Logs</span>
              </button>

              <!-- Delete All Logs -->
              <button
                type="button"
                @click="deleteAllLogs"
                class="flex items-center justify-center space-x-1 p-2 bg-red-50 hover:bg-red-100 border border-red-200 rounded-md transition-colors duration-200"
              >
                <svg class="w-3 h-3 text-red-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path>
                </svg>
                <span class="text-xs font-medium text-red-700">Delete All</span>
              </button>
            </div>
            
            <div class="p-2 bg-blue-50 border border-blue-200 rounded-md">
              <div class="flex items-start space-x-2">
                <svg class="w-3 h-3 text-blue-600 mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
                </svg>
                <p class="text-xs text-blue-800">Logs help diagnose issues. Files are rotated daily and cleaned after 30 days.</p>
              </div>
            </div>
          </div>

          <!-- 按钮组 -->
          <div class="flex justify-end space-x-2 pt-3 border-t border-gray-200">
            <button
              type="button"
              class="px-3 py-1.5 text-xs font-medium text-gray-700 hover:text-gray-900 hover:bg-gray-100 rounded-md transition-all duration-200"
              @click="$emit('update:show', false)"
            >
              Cancel
            </button>
            <button
              type="submit"
              class="px-4 py-1.5 text-xs font-medium text-white bg-gradient-to-r from-blue-500 to-purple-600 hover:from-blue-600 hover:to-purple-700 rounded-md transition-all duration-200 shadow-sm hover:shadow-md"
            >
              Save Settings
            </button>
          </div>
        </form>
      </div>
    </div>
  </div>

  <!-- 确认删除弹窗 -->
  <ConfirmDialog
    v-model:show="showConfirmDialog"
    type="danger"
    title="Delete All Log Files"
    message="确定要删除所有日志文件吗？

此操作不可恢复！"
    confirm-text="Delete All"
    cancel-text="Cancel"
    @confirm="confirmDeleteLogs"
  />
</template> 