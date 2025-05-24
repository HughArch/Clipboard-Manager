<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { Dialog, DialogPanel, DialogTitle, TransitionChild, TransitionRoot } from '@headlessui/vue'
import { XMarkIcon } from '@heroicons/vue/24/outline'
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
  } catch (error) {
    console.error('Failed to load settings:', error)
  }
})

const handleSubmit = async () => {
  try {
    emit('save-settings', settings.value)
    emit('update:show', false)
  } catch (error) {
    console.error('Failed to save settings:', error)
    // 这里可以添加错误提示
  }
}
</script>

<template>
  <div v-if="show" class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center">
    <div class="bg-white rounded-lg shadow-xl w-full max-w-md">
      <div class="p-6">
        <h2 class="text-xl font-semibold mb-4">Settings</h2>
        
        <form @submit.prevent="handleSubmit">
          <!-- 最大历史记录数 -->
          <div class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">
              Max History Items
            </label>
            <input
              v-model.number="settings.max_history_items"
              type="number"
              min="1"
              class="w-full border border-gray-300 rounded-md px-3 py-2"
            />
          </div>

          <!-- 最大保存时间（天） -->
          <div class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">
              Max History Time (days)
            </label>
            <input
              v-model.number="settings.max_history_time"
              type="number"
              min="1"
              class="w-full border border-gray-300 rounded-md px-3 py-2"
            />
          </div>

          <!-- 快捷键 -->
          <div class="mb-4">
            <label class="block text-sm font-medium text-gray-700 mb-1">
              Hotkey
            </label>
            <input
              v-model="settings.hotkey"
              type="text"
              placeholder="e.g. Ctrl+Shift+V"
              class="w-full border border-gray-300 rounded-md px-3 py-2"
            />
          </div>

          <!-- 开机自启动 -->
          <div class="mb-6">
            <label class="flex items-center">
              <input
                v-model="settings.auto_start"
                type="checkbox"
                class="rounded border-gray-300 text-blue-600"
              />
              <span class="ml-2 text-sm text-gray-700">Start with system</span>
            </label>
          </div>

          <!-- 按钮组 -->
          <div class="flex justify-end space-x-3">
            <button
              type="button"
              class="px-4 py-2 text-sm font-medium text-gray-700 hover:bg-gray-100 rounded-md"
              @click="$emit('update:show', false)"
            >
              Cancel
            </button>
            <button
              type="submit"
              class="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md"
            >
              Save
            </button>
          </div>
        </form>
      </div>
    </div>
  </div>
</template> 