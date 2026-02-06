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

const getIconClass = (type: string) => {
  switch (type) {
    case 'success': return 'text-emerald-500'
    case 'error': return 'text-red-500'
    case 'warning': return 'text-amber-500'
    case 'info': return 'text-primary-500'
    default: return 'text-primary-500'
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
      }, message.duration || 4000)
    }
  })
}, { immediate: true, deep: true })
</script>

<template>
  <div class="fixed top-4 right-4 z-[9999] flex flex-col gap-2">
    <TransitionGroup
      name="toast"
      tag="div"
      class="flex flex-col gap-2"
    >
      <div
        v-for="toast in messages"
        :key="toast.id"
        class="toast-modern"
      >
        <component 
          :is="getIcon(toast.type)" 
          :class="['toast-icon', getIconClass(toast.type)]"
        />
        <div class="toast-content">
          <p class="toast-title">{{ toast.title }}</p>
          <p 
            v-if="toast.message" 
            class="toast-message"
          >
            {{ toast.message }}
          </p>
        </div>
        <button
          @click="removeToast(toast.id)"
          class="toast-close"
        >
          <XMarkIcon class="w-4 h-4" />
        </button>
      </div>
    </TransitionGroup>
  </div>
</template>

<style scoped>
.toast-enter-active,
.toast-leave-active {
  transition: all 0.2s ease-out;
}

.toast-enter-from {
  opacity: 0;
  transform: translateX(100%);
}

.toast-leave-to {
  opacity: 0;
  transform: translateX(100%);
}

.toast-move {
  transition: transform 0.2s ease-out;
}
</style>
