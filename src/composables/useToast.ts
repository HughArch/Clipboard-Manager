import { ref } from 'vue'
import type { ToastMessage } from '../components/Toast.vue'

const toastMessages = ref<ToastMessage[]>([])

export function useToast() {
  const addToast = (toast: Omit<ToastMessage, 'id'>) => {
    const id = Date.now().toString() + Math.random().toString(36).substr(2, 9)
    const newToast: ToastMessage = {
      id,
      duration: 5000, // 默认5秒
      ...toast
    }
    toastMessages.value.push(newToast)
    return id
  }

  const removeToast = (id: string) => {
    const index = toastMessages.value.findIndex(toast => toast.id === id)
    if (index > -1) {
      toastMessages.value.splice(index, 1)
    }
  }

  const showSuccess = (title: string, message?: string, duration?: number) => {
    return addToast({ type: 'success', title, message, duration })
  }

  const showError = (title: string, message?: string, duration?: number) => {
    return addToast({ type: 'error', title, message, duration })
  }

  const showWarning = (title: string, message?: string, duration?: number) => {
    return addToast({ type: 'warning', title, message, duration })
  }

  const showInfo = (title: string, message?: string, duration?: number) => {
    return addToast({ type: 'info', title, message, duration })
  }

  const clearAll = () => {
    toastMessages.value = []
  }

  return {
    toastMessages,
    addToast,
    removeToast,
    showSuccess,
    showError,
    showWarning,
    showInfo,
    clearAll
  }
} 