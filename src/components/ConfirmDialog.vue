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
        bgColor: 'bg-red-50 dark:bg-red-900/30',
        confirmButtonClass: 'btn-danger'
      }
    case 'info':
      return {
        iconColor: 'text-primary-500',
        bgColor: 'bg-primary-50 dark:bg-primary-900/30',
        confirmButtonClass: 'btn-primary'
      }
    default: // warning
      return {
        iconColor: 'text-amber-500',
        bgColor: 'bg-amber-50 dark:bg-amber-900/30',
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
      <div class="dialog-box w-80 max-w-[90vw]">
        <!-- Header with Icon -->
        <div class="px-5 pt-5 pb-4">
          <div class="flex items-start gap-3">
            <div :class="['w-8 h-8 rounded-lg flex items-center justify-center flex-shrink-0 mt-0.5', getTypeConfig().bgColor]">
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
            <div class="flex-1 min-w-0">
              <h3 class="dialog-title text-sm">{{ title }}</h3>
              <p class="mt-1.5 text-sm text-base-content/60 leading-relaxed whitespace-pre-line">{{ message }}</p>
            </div>
          </div>
        </div>

        <!-- Actions -->
        <div class="dialog-footer">
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
