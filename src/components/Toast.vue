<script setup lang="ts">
import { watch } from 'vue'
import { CheckCircleIcon, XCircleIcon, ExclamationTriangleIcon, InformationCircleIcon, XMarkIcon } from '@heroicons/vue/24/outline'

export interface ToastMessage {
  id: string
  type: 'success' | 'error' | 'warning' | 'info'
  title: string
  message?: string
  duration?: number
}

const props = defineProps<{
  messages: ToastMessage[]
}>()

const emit = defineEmits<{
  (e: 'remove', id: string): void
}>()

const getIcon = (type: string) => {
  switch (type) {
    case 'success': return CheckCircleIcon
    case 'error': return XCircleIcon
    case 'warning': return ExclamationTriangleIcon
    case 'info': return InformationCircleIcon
    default: return InformationCircleIcon
  }
}

const getColorClasses = (type: string) => {
  switch (type) {
    case 'success': 
      return {
        container: 'bg-green-50 border-green-200',
        icon: 'text-green-400',
        title: 'text-green-800',
        message: 'text-green-600',
        button: 'text-green-400 hover:text-green-600'
      }
    case 'error': 
      return {
        container: 'bg-red-50 border-red-200',
        icon: 'text-red-400',
        title: 'text-red-800',
        message: 'text-red-600',
        button: 'text-red-400 hover:text-red-600'
      }
    case 'warning': 
      return {
        container: 'bg-yellow-50 border-yellow-200',
        icon: 'text-yellow-400',
        title: 'text-yellow-800',
        message: 'text-yellow-600',
        button: 'text-yellow-400 hover:text-yellow-600'
      }
    case 'info': 
    default:
      return {
        container: 'bg-blue-50 border-blue-200',
        icon: 'text-blue-400',
        title: 'text-blue-800',
        message: 'text-blue-600',
        button: 'text-blue-400 hover:text-blue-600'
      }
  }
}

const removeToast = (id: string) => {
  emit('remove', id)
}

// 自动移除功能
watch(() => props.messages, (newMessages) => {
  newMessages.forEach(message => {
    if (message.duration !== 0) { // duration为0表示不自动移除
      setTimeout(() => {
        removeToast(message.id)
      }, message.duration || 5000)
    }
  })
}, { immediate: true, deep: true })
</script>

<template>
  <div class="fixed top-4 right-4 z-[9999] space-y-2 pointer-events-none">
    <TransitionGroup
      name="toast"
      tag="div"
      class="space-y-2"
    >
      <div
        v-for="toast in messages"
        :key="toast.id"
        :class="[
          'w-80 border rounded-lg shadow-lg backdrop-blur-sm pointer-events-auto',
          'transform transition-all duration-300 ease-in-out',
          getColorClasses(toast.type).container
        ]"
      >
        <div class="p-4">
          <div class="flex items-start space-x-3">
            <div class="flex-shrink-0">
              <component 
                :is="getIcon(toast.type)" 
                :class="['h-5 w-5', getColorClasses(toast.type).icon]"
              />
            </div>
            <div class="flex-1">
              <p :class="['text-sm font-medium leading-tight', getColorClasses(toast.type).title]">
                {{ toast.title }}
              </p>
              <p 
                v-if="toast.message" 
                :class="['mt-1 text-sm leading-relaxed', getColorClasses(toast.type).message]"
              >
                {{ toast.message }}
              </p>
            </div>
            <div class="ml-2 flex-shrink-0 flex">
              <button
                @click="removeToast(toast.id)"
                :class="[
                  'inline-flex p-1 rounded-md focus:outline-none focus:ring-2 focus:ring-offset-2 hover:bg-black hover:bg-opacity-10',
                  getColorClasses(toast.type).button
                ]"
              >
                <span class="sr-only">Close</span>
                <XMarkIcon class="h-3 w-3" />
              </button>
            </div>
          </div>
        </div>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: all 0.3s ease;
}

.toast-enter-from {
  opacity: 0;
  transform: translateX(100%) scale(0.95);
}

.toast-leave-to {
  opacity: 0;
  transform: translateX(100%) scale(0.95);
}

.toast-move {
  transition: transform 0.3s ease;
}
</style> 