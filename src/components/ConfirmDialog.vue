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
        confirmButtonClass: 'bg-red-600 hover:bg-red-700 focus:ring-red-500'
      }
    case 'info':
      return {
        iconColor: 'text-blue-600',
        bgColor: 'bg-blue-100',
        confirmButtonClass: 'bg-blue-600 hover:bg-blue-700 focus:ring-blue-500'
      }
    default: // warning
      return {
        iconColor: 'text-amber-600',
        bgColor: 'bg-amber-100',
        confirmButtonClass: 'bg-amber-600 hover:bg-amber-700 focus:ring-amber-500'
      }
  }
}
</script>

<template>
  <Teleport to="body">
    <div 
      v-if="show" 
      class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-[9999] p-3"
      @click="handleBackdropClick"
    >
      <div 
        class="bg-white rounded-lg shadow-xl w-full max-w-xs transform transition-all duration-300 scale-100"
        @click.stop
      >
        <!-- Header with Icon -->
        <div class="px-4 py-3">
          <div class="flex items-center space-x-3">
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
              <h3 class="text-sm font-semibold text-gray-900">{{ title }}</h3>
            </div>
          </div>
        </div>
        
        <!-- Content -->
        <div class="px-4 pb-3">
          <p class="text-xs text-gray-600 leading-relaxed whitespace-pre-line">{{ message }}</p>
        </div>
        
        <!-- Actions -->
        <div class="px-4 py-3 bg-gray-50 rounded-b-lg flex justify-end space-x-2">
          <button
            @click="handleCancel"
            class="px-3 py-1.5 text-xs font-medium text-gray-700 hover:text-gray-900 hover:bg-gray-200 rounded-md transition-all duration-200 focus:outline-none focus:ring-1 focus:ring-gray-300"
          >
            {{ cancelText }}
          </button>
          <button
            @click="handleConfirm"
            :class="[
              'px-4 py-1.5 text-xs font-medium text-white rounded-md transition-all duration-200 focus:outline-none focus:ring-1 focus:ring-offset-1',
              getTypeConfig().confirmButtonClass
            ]"
          >
            {{ confirmText }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
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