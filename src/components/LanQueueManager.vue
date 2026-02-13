<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { logger } from '../composables/useLogger'

interface AppSettings {
  max_history_items: number
  max_history_time: number
  hotkey: string
  auto_start: boolean
  lan_queue_role: string
  lan_queue_host: string
  lan_queue_port: number
  lan_queue_password: string
  lan_queue_name: string
  lan_queue_member_name: string
}

interface LanQueueStatus {
  role: string
  connected: boolean
  host?: string | null
  port?: number | null
  self_id: string
  self_name?: string | null
}

interface LanQueueMember {
  id: string
  name?: string | null
  addr?: string | null
  is_self: boolean
}

defineProps<{
  show: boolean
}>()

const emit = defineEmits<{
  (e: 'update:show', value: boolean): void
  (e: 'show-toast', toast: { type: 'success' | 'error' | 'warning' | 'info', title: string, message?: string, duration?: number }): void
}>()

const settings = ref<AppSettings>({
  max_history_items: 100,
  max_history_time: 30,
  hotkey: 'Ctrl+Shift+V',
  auto_start: false,
  lan_queue_role: 'off',
  lan_queue_host: '',
  lan_queue_port: 21991,
  lan_queue_password: '',
  lan_queue_name: '',
  lan_queue_member_name: ''
})

const lanStatus = ref<LanQueueStatus | null>(null)
const lanMembers = ref<LanQueueMember[]>([])
const lanBusy = ref(false)

let unlistenLanStatus: (() => void) | null = null
let unlistenLanMembers: (() => void) | null = null

const isConnected = computed(() => !!lanStatus.value?.connected)
const isHost = computed(() => lanStatus.value?.role === 'host')
const queueInfo = computed(() => {
  if (!lanStatus.value) return ''
  const host = lanStatus.value.host
  const port = lanStatus.value.port
  if (host && port) {
    return `${host}:${port}`
  }
  if (port) {
    return `端口 ${port}`
  }
  return ''
})

const formatMemberAddr = (member: LanQueueMember): string => {
  if (member.addr) return member.addr
  if (member.id === lanStatus.value?.self_id) return '本机'
  if (lanStatus.value?.host && lanStatus.value?.port) {
    return `${lanStatus.value.host}:${lanStatus.value.port}`
  }
  return '未知'
}

const formatMemberName = (member: LanQueueMember): string => {
  if (member.id === lanStatus.value?.self_id) {
    const selfName = settings.value.lan_queue_member_name?.trim() || lanStatus.value?.self_name?.trim()
    if (selfName) return selfName
  }
  const name = member.name?.trim()
  return name && name.length > 0 ? name : '未命名'
}

const persistSettings = async () => {
  try {
    await invoke('save_settings', { settings: settings.value })
  } catch (error) {
    logger.warn('保存 LAN 设置失败', { error: String(error) })
  }
}

const ensureMemberName = (): boolean => {
  const name = settings.value.lan_queue_member_name?.trim()
  if (!name) {
    emit('show-toast', { type: 'warning', title: '缺少成员名称', message: '请输入成员名称后再加入/创建队列', duration: 4000 })
    return false
  }
  return true
}

onMounted(async () => {
  try {
    const savedSettings = await invoke<AppSettings>('load_settings')
    settings.value = savedSettings
  } catch (error) {
    logger.warn('加载 LAN 设置失败', { error: String(error) })
  }

  try {
    lanStatus.value = await invoke<LanQueueStatus>('lan_queue_status')
  } catch (error) {
    logger.warn('获取 LAN 队列状态失败', { error: String(error) })
  }

  unlistenLanStatus = await listen<LanQueueStatus>('lan-queue-status', (event) => {
    lanStatus.value = event.payload
  })

  unlistenLanMembers = await listen<LanQueueMember[]>('lan-queue-members', (event) => {
    const members = event.payload || []
    const selfId = lanStatus.value?.self_id
    const selfName = settings.value.lan_queue_member_name?.trim()
    lanMembers.value = members.map(member => {
      if (selfId && member.id === selfId && selfName) {
        return { ...member, name: member.name?.trim() ? member.name : selfName }
      }
      return member
    })
  })
})

onUnmounted(() => {
  if (unlistenLanStatus) {
    unlistenLanStatus()
    unlistenLanStatus = null
  }
  if (unlistenLanMembers) {
    unlistenLanMembers()
    unlistenLanMembers = null
  }
})

const startLanHost = async () => {
  if (!ensureMemberName()) return
  lanBusy.value = true
  try {
    await persistSettings()
    lanStatus.value = await invoke<LanQueueStatus>('lan_queue_start_host', {
      port: settings.value.lan_queue_port,
      password: settings.value.lan_queue_password,
      queueName: null,
      memberName: settings.value.lan_queue_member_name || null
    })
    settings.value.lan_queue_role = 'host'
    await persistSettings()
    emit('show-toast', { type: 'success', title: '队列已创建', message: '主机监听已启动', duration: 3000 })
  } catch (error) {
    emit('show-toast', { type: 'error', title: '创建失败', message: String(error), duration: 5000 })
  } finally {
    lanBusy.value = false
  }
}

const joinLanQueue = async () => {
  if (!ensureMemberName()) return
  lanBusy.value = true
  try {
    await persistSettings()
    lanStatus.value = await invoke<LanQueueStatus>('lan_queue_join', {
      host: settings.value.lan_queue_host,
      port: settings.value.lan_queue_port,
      password: settings.value.lan_queue_password,
      memberName: settings.value.lan_queue_member_name || null
    })
    settings.value.lan_queue_role = 'client'
    await persistSettings()
    emit('show-toast', { type: 'success', title: '加入成功', message: '已加入局域网队列', duration: 3000 })
  } catch (error) {
    emit('show-toast', { type: 'error', title: '加入失败', message: String(error), duration: 5000 })
  } finally {
    lanBusy.value = false
  }
}

const leaveLanQueue = async () => {
  lanBusy.value = true
  try {
    await invoke('lan_queue_leave')
    settings.value.lan_queue_role = 'off'
    await persistSettings()
    lanStatus.value = await invoke<LanQueueStatus>('lan_queue_status')
    lanMembers.value = []
    emit('show-toast', { type: 'success', title: '已退出队列', message: 'LAN 队列已断开', duration: 3000 })
  } catch (error) {
    emit('show-toast', { type: 'error', title: '退出失败', message: String(error), duration: 5000 })
  } finally {
    lanBusy.value = false
  }
}
</script>

<template>
  <Transition name="dialog">
    <div v-if="show" class="dialog-overlay" @click.self="$emit('update:show', false)">
      <div class="bg-white rounded-2xl shadow-xl shadow-black/10 w-full max-w-lg max-h-[90vh] flex flex-col overflow-hidden">
        <div class="px-5 py-4 border-b border-gray-100 flex-shrink-0">
          <div class="flex items-center gap-3">
            <div class="w-9 h-9 bg-primary-100 rounded-xl flex items-center justify-center">
              <svg class="w-5 h-5 text-primary-600" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 17l4 4 4-4m-4-5V3m-7 7h14"></path>
              </svg>
            </div>
            <h2 class="text-base font-semibold text-gray-900">局域网队列管理</h2>
          </div>
        </div>

        <div class="p-5 flex-1 overflow-y-auto space-y-4">
          <div v-if="isConnected" class="p-3 bg-gray-50 rounded-xl">
            <div class="flex items-center justify-between">
              <div>
                <p class="text-sm font-medium text-gray-700">已加入队列</p>
                <p class="text-xs text-gray-500">
                  {{ isHost ? '主机' : '客户端' }}
                  <span v-if="queueInfo">· {{ queueInfo }}</span>
                </p>
              </div>
              <span v-if="lanBusy" class="text-xs text-primary-600">处理中…</span>
            </div>
          </div>

          <div v-if="!isConnected" class="space-y-4">
            <div class="grid grid-cols-2 gap-4">
              <div class="space-y-1.5 col-span-2">
                <label class="block text-sm font-medium text-gray-700">成员名称</label>
                <input
                  v-model="settings.lan_queue_member_name"
                  type="text"
                  placeholder="例如：Alice"
                  class="input input-sm"
                />
              </div>
            </div>

            <div class="grid grid-cols-2 gap-4">
              <div class="space-y-1.5">
                <label class="block text-sm font-medium text-gray-700">主机地址</label>
                <input
                  v-model="settings.lan_queue_host"
                  type="text"
                  placeholder="192.168.1.2"
                  class="input input-sm"
                />
              </div>
              <div class="space-y-1.5">
                <label class="block text-sm font-medium text-gray-700">端口</label>
                <input
                  v-model.number="settings.lan_queue_port"
                  type="number"
                  min="1"
                  max="65535"
                  class="input input-sm"
                />
              </div>
            </div>

            <div class="space-y-1.5">
              <label class="block text-sm font-medium text-gray-700">队列密码</label>
              <input
                v-model="settings.lan_queue_password"
                type="password"
                placeholder="加入队列需要密码"
                class="input input-sm"
              />
            </div>
          </div>

          <div class="flex flex-wrap gap-2">
            <button
              v-if="!isConnected"
              type="button"
              class="btn btn-sm btn-primary"
              :disabled="lanBusy"
              @click="startLanHost"
            >
              创建队列
            </button>
            <button
              v-if="!isConnected"
              type="button"
              class="btn btn-sm btn-secondary"
              :disabled="lanBusy"
              @click="joinLanQueue"
            >
              加入队列
            </button>
            <button
              v-if="isConnected"
              type="button"
              class="btn btn-sm btn-danger"
              :disabled="lanBusy"
              @click="leaveLanQueue"
            >
              退出队列
            </button>
          </div>

          <div class="space-y-2">
            <p class="text-sm font-medium text-gray-700">成员列表</p>
            <div v-if="lanMembers.length === 0" class="text-xs text-gray-500">暂无成员</div>
            <div v-else class="space-y-1">
              <div
                v-for="member in lanMembers"
                :key="member.id"
                class="flex items-center justify-between text-sm text-gray-700 bg-gray-50 rounded-lg px-3 py-2"
              >
                <div class="flex flex-col">
                  <span class="font-medium">{{ formatMemberName(member) }}</span>
                  <span class="text-xs text-gray-500">{{ formatMemberAddr(member) }}</span>
                </div>
                <span v-if="member.id === lanStatus?.self_id" class="text-xs text-primary-600">我</span>
              </div>
            </div>
          </div>
        </div>

        <div class="px-5 py-4 bg-gray-50 border-t border-gray-100 flex justify-end gap-2 flex-shrink-0">
          <button
            type="button"
            class="btn btn-sm btn-ghost"
            @click="$emit('update:show', false)"
          >
            关闭
          </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
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
