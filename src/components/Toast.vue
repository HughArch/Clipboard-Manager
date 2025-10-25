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

const getAlertClass = (type: string) => {
  switch (type) {
    case 'success': return 'alert-success'
    case 'error': return 'alert-error'
    case 'warning': return 'alert-warning'
    case 'info': return 'alert-info'
    default: return 'alert-info'
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
  <div class="toast toast-top toast-end z-[9999]">
    <TransitionGroup
      name="toast"
      tag="div"
      class="space-y-2"
    >
      <div
        v-for="toast in messages"
        :key="toast.id"
        :class="[
          'alert w-80 shadow-lg',
          getAlertClass(toast.type)
        ]"
      >
        <div class="flex items-start space-x-3">
          <div class="flex-shrink-0">
            <component 
              :is="getIcon(toast.type)" 
              class="h-5 w-5"
            />
          </div>
          <div class="flex-1">
            <p class="text-sm font-medium leading-tight">
              {{ toast.title }}
            </p>
            <p 
              v-if="toast.message" 
              class="mt-1 text-sm leading-relaxed opacity-80"
            >
              {{ toast.message }}
            </p>
          </div>
          <div class="ml-2 flex-shrink-0 flex">
            <button
              @click="removeToast(toast.id)"
              class="btn btn-sm btn-circle btn-ghost"
            >
              <XMarkIcon class="h-3 w-3" />
            </button>
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