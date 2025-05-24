<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { Dialog, DialogPanel, DialogTitle, TransitionChild, TransitionRoot } from '@headlessui/vue'
import { XMarkIcon } from '@heroicons/vue/24/outline'
import { invoke } from '@tauri-apps/api/core'

const props = defineProps<{
  show: boolean
}>()

const emit = defineEmits<{
  (e: 'update:show', value: boolean): void
}>()

const settings = ref({
  max_history_items: 100,
  max_history_time: 24, // hours
  hotkey: 'Ctrl+Shift+V',
  auto_start: true
})

async function loadSettings() {
  try {
    const result = await invoke('load_settings')
    if (result && typeof result === 'object') {
      Object.assign(settings.value, result as object)
    }
  } catch (e) {
    // ignore if not found
    // 可选：console.warn('加载设置失败，使用默认值', e)
  }
}

async function saveSettings() {
  try {
    await invoke('save_settings', { settings: settings.value })
    emit('update:show', false)
  } catch (e) {
    // 打印详细错误
    console.error('保存失败', e)
    alert('保存失败: ' + (e && e.toString ? e.toString() : e))
  }
}

onMounted(loadSettings)
</script>

<template>
  <TransitionRoot appear :show="show" as="template">
    <Dialog as="div" @close="emit('update:show', false)" class="relative z-10">
      <TransitionChild
        as="template"
        enter="duration-300 ease-out"
        enter-from="opacity-0"
        enter-to="opacity-100"
        leave="duration-200 ease-in"
        leave-from="opacity-100"
        leave-to="opacity-0"
      >
        <div class="fixed inset-0 bg-black bg-opacity-25" />
      </TransitionChild>

      <div class="fixed inset-0 overflow-y-auto">
        <div class="flex min-h-full items-center justify-center p-4 text-center">
          <TransitionChild
            as="template"
            enter="duration-300 ease-out"
            enter-from="opacity-0 scale-95"
            enter-to="opacity-100 scale-100"
            leave="duration-200 ease-in"
            leave-from="opacity-100 scale-100"
            leave-to="opacity-0 scale-95"
          >
            <DialogPanel class="w-full max-w-md transform overflow-hidden rounded-2xl bg-white p-6 text-left align-middle shadow-xl transition-all">
              <DialogTitle as="h3" class="text-lg font-medium leading-6 text-gray-900 flex justify-between items-center">
                Settings
                <button
                  type="button"
                  class="text-gray-400 hover:text-gray-500"
                  @click="emit('update:show', false)"
                >
                  <XMarkIcon class="h-6 w-6" />
                </button>
              </DialogTitle>

              <div class="mt-4 space-y-4">
                <div>
                  <label class="block text-sm font-medium text-gray-700">最大保留条目</label>
                  <input
                    v-model.number="settings.max_history_items"
                    type="number"
                    class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500"
                  />
                </div>

                <div>
                  <label class="block text-sm font-medium text-gray-700">最大保留时长 (时)</label>
                  <input
                    v-model.number="settings.max_history_time"
                    type="number"
                    class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500"
                  />
                </div>

                <div>
                  <label class="block text-sm font-medium text-gray-700">快捷键</label>
                  <input
                    v-model="settings.hotkey"
                    type="text"
                    class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500"
                    readonly
                  />
                </div>

                <div class="flex items-center">
                  <input
                    v-model="settings.auto_start"
                    type="checkbox"
                    class="h-4 w-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
                  />
                  <label class="ml-2 block text-sm text-gray-900">开机启动</label>
                </div>
              </div>

              <div class="mt-6 flex justify-end space-x-3">
                <button
                  type="button"
                  class="btn btn-secondary"
                  @click="emit('update:show', false)"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  class="btn btn-primary"
                  @click="saveSettings"
                >
                  Save
                </button>
              </div>
            </DialogPanel>
          </TransitionChild>
        </div>
      </div>
    </Dialog>
  </TransitionRoot>
</template> 