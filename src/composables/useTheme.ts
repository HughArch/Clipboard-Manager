import { ref } from 'vue'

export type Theme = 'light' | 'dark' | 'system'

const THEME_KEY = 'clipboard-manager-theme'

// 响应式主题状态
const currentTheme = ref<Theme>('light')
const isDark = ref(false)

// 获取系统主题偏好
const getSystemTheme = (): 'light' | 'dark' => {
  if (typeof window === 'undefined') return 'light'
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

// 应用主题到 DOM
const applyTheme = (theme: Theme) => {
  if (typeof document === 'undefined') return

  const resolvedTheme = theme === 'system' ? getSystemTheme() : theme

  // 设置 DaisyUI 的 data-theme 属性
  document.documentElement.setAttribute('data-theme', resolvedTheme)

  // 同时设置 class 用于 Tailwind dark: 前缀
  if (resolvedTheme === 'dark') {
    document.documentElement.classList.add('dark')
  } else {
    document.documentElement.classList.remove('dark')
  }

  isDark.value = resolvedTheme === 'dark'
}

// 初始化主题（从 localStorage 读取）
const initTheme = () => {
  if (typeof window === 'undefined') return

  const savedTheme = localStorage.getItem(THEME_KEY) as Theme | null
  if (savedTheme && ['light', 'dark', 'system'].includes(savedTheme)) {
    currentTheme.value = savedTheme
  } else {
    // 默认跟随系统
    currentTheme.value = 'system'
  }

  applyTheme(currentTheme.value)

  // 监听系统主题变化
  window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
    if (currentTheme.value === 'system') {
      applyTheme('system')
    }
  })
}

// 切换主题
const setTheme = (theme: Theme) => {
  currentTheme.value = theme
  localStorage.setItem(THEME_KEY, theme)
  applyTheme(theme)
}

// 在主题间循环切换
const toggleTheme = () => {
  const themes: Theme[] = ['light', 'dark', 'system']
  const currentIndex = themes.indexOf(currentTheme.value)
  const nextIndex = (currentIndex + 1) % themes.length
  setTheme(themes[nextIndex])
}

// 获取主题显示名称
const getThemeLabel = (theme: Theme): string => {
  switch (theme) {
    case 'light': return '浅色'
    case 'dark': return '深色'
    case 'system': return '跟随系统'
    default: return '浅色'
  }
}

// 获取主题图标
const getThemeIcon = (theme: Theme): string => {
  switch (theme) {
    case 'light': return '☀️'
    case 'dark': return '🌙'
    case 'system': return '💻'
    default: return '☀️'
  }
}

export function useTheme() {
  return {
    currentTheme,
    isDark,
    initTheme,
    setTheme,
    toggleTheme,
    getThemeLabel,
    getThemeIcon,
    getSystemTheme
  }
}
