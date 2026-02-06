<script setup lang="ts">
interface Props {
  show: boolean
  title: string
  message: string
  confirmText?: string
  cancelText?: string
  type?: 'warning' | 'danger' | 'info'
}

const props = withDefaults(defineProps<Props>(), {
  confirmText: 'Confirm',
  cancelText: 'Cancel',
  type: 'warning'
})

const emit = defineEmits<{
  (e: 'update:show', value: boolean): void
  (e: 'confirm'): void
  (e: 'cancel'): void
}>()

const handleConfirm = () => {
  emit('confirm')
  emit('update:show', false)
}

const handleCancel = () => {
  emit('cancel')
  emit('update:show', false)
}

const handleBackdropClick = (event: MouseEvent) => {
  if (event.target === event.currentTarget) {
    handleCancel()
  }
}

// 根据类型获取样式配置
const getTypeConfig = () => {
  switch (props.type) {
    case 'danger':
      return {
        iconColor: 'text-red-500',
        bgColor: 'bg-red-50',
        confirmButtonClass: 'btn-danger'
      }
    case 'info':
      return {
        iconColor: 'text-primary-500',
        bgColor: 'bg-primary-50',
        confirmButtonClass: 'btn-primary'
      }
    default: // warning
      return {
        iconColor: 'text-amber-500',
        bgColor: 'bg-amber-50',
        confirmButtonClass: 'btn-warning'
      }
  }
}
</script>

<template>
  <Transition name="dialog">
    <div 
      v-if="show"
      class="dialog-overlay"
      @click="handleBackdropClick"
    >
      <div class="bg-white rounded-2xl shadow-xl shadow-black/10 w-80 max-w-[90vw] overflow-hidden">
        <!-- Header with Icon -->
        <div class="px-5 pt-5 pb-4">
          <div class="flex items-start gap-4">
            <div :class="['w-10 h-10 rounded-xl flex items-center justify-center flex-shrink-0', getTypeConfig().bgColor]">
              <!-- Danger Icon -->
              <svg 
                v-if="type === 'danger'" 
                :class="getTypeConfig().iconColor" 
                class="w-5 h-5" 
                fill="none" 
                stroke="currentColor" 
                viewBox="0 0 24 24"
              >
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L3.732 16.5c-.77.833.192 2.5 1.732 2.5z"></path>
              </svg>
              
              <!-- Info Icon -->
              <svg 
                v-else-if="type === 'info'" 
                :class="getTypeConfig().iconColor" 
                class="w-5 h-5" 
                fill="none" 
                stroke="currentColor" 
                viewBox="0 0 24 24"
              >
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"></path>
              </svg>
              
              <!-- Warning Icon (default) -->
              <svg 
                v-else 
                :class="getTypeConfig().iconColor" 
                class="w-5 h-5" 
                fill="none" 
                stroke="currentColor" 
                viewBox="0 0 24 24"
              >
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L3.732 16.5c-.77.833.192 2.5 1.732 2.5z"></path>
              </svg>
            </div>
            <div class="flex-1 pt-1">
              <h3 class="text-sm font-semibold text-gray-900">{{ title }}</h3>
              <p class="mt-2 text-sm text-gray-500 leading-relaxed whitespace-pre-line">{{ message }}</p>
            </div>
          </div>
        </div>
        
        <!-- Actions -->
        <div class="px-5 py-4 bg-gray-50 flex justify-end gap-2">
          <button
            @click="handleCancel"
            class="btn btn-sm btn-ghost"
          >
            {{ cancelText }}
          </button>
          <button
            @click="handleConfirm"
            :class="['btn btn-sm', getTypeConfig().confirmButtonClass]"
          >
            {{ confirmText }}
          </button>
        </div>
      </div>
    </div>
  </Transition>
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
