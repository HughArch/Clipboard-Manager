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
        iconColor: 'text-red-600',
        bgColor: 'bg-red-100',
        confirmButtonClass: 'btn-error'
      }
    case 'info':
      return {
        iconColor: 'text-blue-600',
        bgColor: 'bg-blue-100',
        confirmButtonClass: 'btn-info'
      }
    default: // warning
      return {
        iconColor: 'text-amber-600',
        bgColor: 'bg-amber-100',
        confirmButtonClass: 'btn-warning'
      }
  }
}
</script>

<template>
  <dialog 
    :id="`confirm-dialog-${Math.random().toString(36).substr(2, 9)}`"
    :class="['modal', { 'modal-open': show }]"
  >
    <div class="modal-box w-80 max-w-xs">
      <!-- Header with Icon -->
      <div class="flex items-center space-x-3 mb-4">
        <div :class="['w-8 h-8 rounded-full flex items-center justify-center flex-shrink-0', getTypeConfig().bgColor]">
          <!-- Danger Icon -->
          <svg 
            v-if="type === 'danger'" 
            :class="getTypeConfig().iconColor" 
            class="w-4 h-4" 
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
            class="w-4 h-4" 
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
            class="w-4 h-4" 
            fill="none" 
            stroke="currentColor" 
            viewBox="0 0 24 24"
          >
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L3.732 16.5c-.77.833.192 2.5 1.732 2.5z"></path>
          </svg>
        </div>
        <div class="flex-1">
          <h3 class="text-sm font-semibold">{{ title }}</h3>
        </div>
      </div>
      
      <!-- Content -->
      <div class="mb-4">
        <p class="text-xs leading-relaxed whitespace-pre-line opacity-80">{{ message }}</p>
      </div>
      
      <!-- Actions -->
      <div class="modal-action">
        <button
          @click="handleCancel"
          class="btn btn-sm btn-ghost"
        >
          {{ cancelText }}
        </button>
        <button
          @click="handleConfirm"
          :class="[
            'btn btn-sm',
            getTypeConfig().confirmButtonClass
          ]"
        >
          {{ confirmText }}
        </button>
      </div>
    </div>
    
    <!-- Modal backdrop -->
    <form method="dialog" class="modal-backdrop" @click="handleBackdropClick">
      <button type="button">close</button>
    </form>
  </dialog>
</template>

<style scoped>
/* 入场动画 */
.v-enter-active, .v-leave-active {
  transition: all 0.3s ease;
}

.v-enter-from, .v-leave-to {
  opacity: 0;
  transform: scale(0.95);
}

.v-enter-to, .v-leave-from {
  opacity: 1;
  transform: scale(1);
}
</style>