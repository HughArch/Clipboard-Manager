<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { logger } from '../composables/useLogger'
import ConfirmDialog from './ConfirmDialog.vue'
import { getFormattedVersion } from '../config/version'

// 动态获取版本信息
const appVersion = ref(getFormattedVersion())

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

// 跨平台快捷键检测工具函数
const isMac = () => navigator.platform.toLowerCase().includes('mac')
const getDefaultModifierKey = () => isMac() ? 'Cmd' : 'Ctrl'

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
        // 在 macOS 上，Meta 键应该显示为 Cmd；在其他系统上，可能是 Windows 键，但通常在快捷键中用 Ctrl 代替
        const metaKeyName = isMac() ? 'Cmd' : 'Ctrl'
        if (!modifiers.includes(metaKeyName)) modifiers.push(metaKeyName)
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
    const defaultModifier = getDefaultModifierKey()
    modifiers.push(defaultModifier, 'Shift')
  }
  
  // 组合成快捷键字符串
  if (mainKey) {
    return [...modifiers, mainKey].join('+')
  }
  
  // 默认值也应该是跨平台的
  return `${getDefaultModifierKey()}+Shift+V`
}

const handleKeyDown = (event: KeyboardEvent) => {
  if (!isRecording.value) return
  
  event.preventDefault()
  event.stopPropagation()
  
  // macOS 特殊处理：阻止 Alt 键
  if (isMac() && event.altKey) {
    // 停止录制并显示错误
    stopRecording()
    emit('show-toast', {
      type: 'warning',
      title: 'Unsupported Key',
      message: 'macOS does not support Alt/Option key for global shortcuts. Please use Cmd+Shift+V or Ctrl+Shift+V.',
      duration: 6000
    })
    return
  }
  
  // 记录按下的键 - 跨平台支持
  if (event.ctrlKey) recordingKeys.value.add('Ctrl')
  if (!isMac() && event.altKey) recordingKeys.value.add('Alt') // 只在非 macOS 上允许 Alt
  if (event.shiftKey) recordingKeys.value.add('Shift')
  if (event.metaKey) {
    // 在 macOS 上 metaKey 是 Cmd 键，在其他系统上是 Windows 键
    // 为了快捷键的兼容性，我们将其记录为 Meta，后续在 generateHotkeyString 中处理
    recordingKeys.value.add('Meta')
  }
  
  // 记录主键
  if (event.key && event.key.length === 1) {
    recordingKeys.value.add(event.key)
  } else if (event.key && event.key.startsWith('F') && event.key.length <= 3) {
    recordingKeys.value.add(event.key)
  }
  
  // 如果有修饰键+主键，自动停止录制
  const hasModifier = event.ctrlKey || (!isMac() && event.altKey) || event.shiftKey || event.metaKey
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
  
  if (error.includes('does not support Option/Alt') || error.includes('does not support Alt')) {
    return {
      title: 'Unsupported Key Combination',
      message: 'macOS does not support Alt/Option key for global shortcuts. Please use Cmd+Shift+V or Ctrl+Shift+V instead.'
    }
  }
  
  if (error.includes('Invalid hotkey format')) {
    return {
      title: 'Invalid Format',
      message: `Please use format like: ${getDefaultModifierKey()}+Shift+V`
    }
  }
  
  return {
    title: 'Registration Failed',
    message: 'Please try a different hotkey combination'
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
    emit('show-toast', { type: 'success', title: '保存设置', message: '所有设置已成功保存！', duration: 3000 })
    emit('save-settings', settings.value)
    emit('update:show', false)
    
  } catch (error) {
    console.error('Failed to save settings:', error)
    emit('show-toast', {
      type: 'error',
      title: '保存失败',
      message: '设置无法保存，请重试。',
      duration: 5000
    })
  }
}
</script>

<template>
  <Transition name="dialog">
    <div v-if="show" class="dialog-overlay" @click.self="$emit('update:show', false)">
      <div class="bg-white rounded-2xl shadow-xl shadow-black/10 w-full max-w-md max-h-[90vh] flex flex-col overflow-hidden">
        <!-- Header -->
        <div class="px-5 py-4 border-b border-gray-100 flex-shrink-0">
          <div class="flex items-center gap-3">
            <div class="w-9 h-9 bg-primary-100 rounded-xl flex items-center justify-center">
              <svg class="w-5 h-5 text-primary-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"></path>
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"></path>
              </svg>
            </div>
            <h2 class="text-base font-semibold text-gray-900">设置</h2>
          </div>
        </div>

        <!-- Content -->
        <div class="p-5 flex-1 overflow-y-auto">
          <form @submit.prevent="handleSubmit" class="space-y-6" id="settings-form">
            <!-- General Settings Section -->
            <div class="space-y-4">
              <h3 class="text-xs font-semibold text-gray-500 uppercase tracking-wider">通用设置</h3>
              
              <!-- 最大历史记录数和时间 - 一行两列 -->
              <div class="grid grid-cols-2 gap-4">
                <div class="space-y-1.5">
                  <label class="block text-sm font-medium text-gray-700">最大历史记录数</label>
                  <input
                    v-model.number="settings.max_history_items"
                    type="number"
                    min="1"
                    class="input input-sm"
                  />
                </div>
                <div class="space-y-1.5">
                  <label class="block text-sm font-medium text-gray-700">历史保留天数</label>
                  <input
                    v-model.number="settings.max_history_time"
                    type="number"
                    min="1"
                    class="input input-sm"
                  />
                </div>
              </div>

              <!-- 快捷键 -->
              <div class="space-y-1.5">
                <label class="block text-sm font-medium text-gray-700">全局热键</label>
                <div class="relative">
                  <input
                    ref="hotkeyInputRef"
                    v-model="settings.hotkey"
                    type="text"
                    :placeholder="isRecording ? 'Press keys...' : `e.g. ${getDefaultModifierKey()}+Shift+V`"
                    :readonly="isRecording"
                    class="input input-sm pr-20"
                    :class="{ 'ring-2 ring-primary-500/20 border-primary-500': isRecording }"
                    @keydown="handleKeyDown"
                    @keyup="handleKeyUp"
                    @blur="isRecording && stopRecording()"
                  />
                  <button
                    type="button"
                    @click="isRecording ? stopRecording() : startRecording()"
                    :class="[
                      'btn btn-xs absolute right-1.5 top-1/2 -translate-y-1/2',
                      isRecording ? 'btn-danger' : 'btn-secondary'
                    ]"
                  >
                    {{ isRecording ? '停止' : '录制' }}
                  </button>
                </div>
                <p v-if="isRecording && getRecordingPreview()" class="text-xs text-primary-600">
                  预览: {{ getRecordingPreview() }}
                </p>
              </div>

              <!-- 开机自启动 -->
              <label class="flex items-center justify-between p-3 bg-gray-50 rounded-xl cursor-pointer hover:bg-gray-100 transition-colors duration-200">
                <span class="text-sm font-medium text-gray-700">开机自启动</span>
                <input
                  v-model="settings.auto_start"
                  type="checkbox"
                  class="toggle-modern"
                />
              </label>
            </div>

            <!-- Log Management Section -->
            <div class="space-y-4">
              <h3 class="text-xs font-semibold text-gray-500 uppercase tracking-wider">日志管理</h3>
              
              <div class="grid grid-cols-2 gap-3">
                <!-- Open Log Folder -->
                <button
                  type="button"
                  @click="openLogFolder"
                  class="btn btn-sm btn-secondary gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-5l-2-2H5a2 2 0 00-2 2z"></path>
                  </svg>
                  打开文件夹
                </button>

                <!-- Delete All Logs -->
                <button
                  type="button"
                  @click="deleteAllLogs"
                  class="btn btn-sm btn-danger-ghost gap-2"
                >
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path>
                  </svg>
                  删除日志
                </button>
              </div>
              
              <div class="p-3 bg-primary-50 rounded-xl">
                <div class="flex items-start gap-2">
                  <svg class="w-4 h-4 text-primary-600 mt-0.5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
                  </svg>
                  <p class="text-xs text-primary-700">日志文件用于诊断问题。每天会自动轮换并在30天后清理。</p>
                </div>
              </div>
            </div>

            <!-- 版本信息 -->
            <div class="space-y-4">
              <h3 class="text-xs font-semibold text-gray-500 uppercase tracking-wider">关于</h3>
              
              <div class="p-4 bg-gray-50 rounded-xl">
                <div class="flex items-center justify-between">
                  <div class="flex items-center gap-3">
                    <div class="w-10 h-10 bg-primary-100 rounded-xl flex items-center justify-center">
                      <svg class="w-5 h-5 text-primary-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"></path>
                      </svg>
                    </div>
                    <div>
                      <p class="text-sm font-medium text-gray-900">剪贴板管理器</p>
                      <p class="text-xs text-gray-500">版本 {{ appVersion }}</p>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </form>
        </div>

        <!-- Footer -->
        <div class="px-5 py-4 bg-gray-50 border-t border-gray-100 flex justify-end gap-2 flex-shrink-0">
          <button
            type="button"
            class="btn btn-sm btn-ghost"
            @click="$emit('update:show', false)"
          >
            取消
          </button>
          <button
            type="submit"
            form="settings-form"
            class="btn btn-sm btn-primary"
          >
            保存设置
          </button>
        </div>
      </div>
    </div>
  </Transition>

  <!-- 确认删除弹窗 -->
  <ConfirmDialog
    v-model:show="showConfirmDialog"
    type="danger"
    title="删除所有日志"
    message="确定要删除所有日志文件吗？

此操作不可恢复！"
    confirm-text="删除"
    cancel-text="取消"
    @confirm="confirmDeleteLogs"
  />
</template>

<style scoped>
/* Dialog transition */
.dialog-enter-active,
.dialog-leave-active {
  transition: all 0.2s ease-out;
}

.dialog-enter-from,
.dialog-leave-to {
  opacity: 0;
}

.dialog-enter-from > div,
.dialog-leave-to > div {
  transform: scale(0.95);
  opacity: 0;
}

.dialog-enter-to > div,
.dialog-leave-from > div {
  transform: scale(1);
  opacity: 1;
}
</style>
