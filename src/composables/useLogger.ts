import { invoke } from '@tauri-apps/api/core'

export enum LogLevel {
  ERROR = 'error',
  WARN = 'warn', 
  INFO = 'info',
  DEBUG = 'debug'
}

export interface LogContext {
  component?: string
  action?: string
  [key: string]: any
}

class Logger {
  private buffer: Array<{ level: string; message: string; context?: string }> = []
  private flushTimer: ReturnType<typeof setInterval> | null = null
  private isFlushingLocal = false
  private maxRetries = 3
  private isDevelopment = import.meta.env.DEV || process.env.DEV === 'true' // 支持多种开发环境检测方式

  constructor() {
    this.init()
  }

  private init() {
    // 启动定时刷新 - 降低到1秒以获得更好的实时性
    this.flushTimer = setInterval(() => {
      this.flush()
    }, 1000) // 1秒刷新一次
    
    // 页面卸载时确保日志被写入和清理定时器
    window.addEventListener('beforeunload', () => {
      this.destroy()
    })

    // 捕获未处理的错误（包含堆栈跟踪）
    window.addEventListener('error', (event) => {
      this.error('Uncaught Error', {
        message: event.message,
        filename: event.filename,
        line: event.lineno,
        column: event.colno,
        stack: event.error?.stack || 'No stack trace available'
      })
    })

    // 捕获未处理的Promise拒绝
    window.addEventListener('unhandledrejection', (event) => {
      this.error('Unhandled Promise Rejection', {
        reason: event.reason,
        stack: event.reason?.stack || 'No stack trace available',
        promise: event.promise.toString()
      })
    })
  }

  // 内部调试日志方法
  private logInternal(level: 'debug' | 'warn' | 'error', message: string, ...args: any[]) {
    if (this.isDevelopment) {
      const prefix = `📋 [Logger Internal]`
      switch (level) {
        case 'debug':
          console.log(`${prefix} ${message}`, ...args)
          break
        case 'warn':
          console.warn(`${prefix} ${message}`, ...args)
          break
        case 'error':
          console.error(`${prefix} ${message}`, ...args)
          break
      }
    }
  }

  private async flush() {
    if (this.buffer.length === 0 || this.isFlushingLocal) return
    
    this.logInternal('debug', `Starting flush of ${this.buffer.length} logs`)
    this.isFlushingLocal = true
    const logsToFlush = [...this.buffer] // 创建副本
    
    try {
      // 逐条发送日志，而不是批量发送
      const successfullyFlushed: number[] = []
      
      for (let i = 0; i < logsToFlush.length; i++) {
        const log = logsToFlush[i]
        let retries = 0
        
        while (retries < this.maxRetries) {
          try {
            this.logInternal('debug', `Sending log ${i + 1}/${logsToFlush.length}: ${log.level} - ${log.message.substring(0, 50)}...`)
            await invoke('write_frontend_log', {
              level: log.level,
              message: log.message,
              context: log.context
            })
            this.logInternal('debug', `Successfully sent log ${i + 1}`)
            successfullyFlushed.push(i)
            break
          } catch (error) {
            retries++
            this.logInternal('warn', `Failed to send log ${i + 1}, attempt ${retries}/${this.maxRetries}:`, error)
            if (retries >= this.maxRetries) {
              this.logInternal('error', `Failed to flush log after ${this.maxRetries} retries: ${log.message}`, error)
            } else {
              // 短暂延迟后重试
              await new Promise(resolve => setTimeout(resolve, 100 * retries))
            }
          }
        }
      }
      
      // 只移除成功发送的日志
      if (successfullyFlushed.length > 0) {
        this.logInternal('debug', `Successfully flushed ${successfullyFlushed.length}/${logsToFlush.length} logs`)
        // 倒序移除，避免索引问题
        for (let i = successfullyFlushed.length - 1; i >= 0; i--) {
          this.buffer.splice(successfullyFlushed[i], 1)
        }
        this.logInternal('debug', `Buffer size after flush: ${this.buffer.length}`)
      } else {
        this.logInternal('warn', `No logs were successfully flushed, buffer size remains: ${this.buffer.length}`)
      }
      
    } catch (error) {
      this.logInternal('error', 'Failed to flush logs:', error)
    } finally {
      this.isFlushingLocal = false
      this.logInternal('debug', 'Flush completed')
    }
  }

  // 同步刷新方法（用于页面卸载）
  private flushSync() {
    if (this.buffer.length === 0) return
    
    // 在页面卸载时，使用同步方式（虽然可能不完美，但比异步更可靠）
    this.logInternal('debug', `Flushing remaining logs on page unload: ${this.buffer.length}`)
    this.flush().catch((error) => this.logInternal('error', 'Failed to flush on unload:', error))
  }

  // 添加立即刷新方法
  private async flushImmediately() {
    // 对于重要日志，立即刷新而不等待定时器
    try {
      await this.flush()
    } catch (error) {
      this.logInternal('error', 'Failed to flush immediately:', error)
    }
  }

  // 清理资源的方法
  public destroy() {
    this.logInternal('debug', 'Destroying logger and cleaning up resources')
    
    // 清理定时器
    if (this.flushTimer) {
      clearInterval(this.flushTimer)
      this.flushTimer = null
      this.logInternal('debug', 'Flush timer cleared')
    }
    
    // 最后一次同步刷新
    this.flushSync()
  }

  error(message: string, context?: LogContext) {
    const timestamp = new Date().toISOString()
    const logMessage = `[${timestamp}] ${message}`
    
    // 自动获取调用栈信息
    const stack = new Error().stack
    const enhancedContext = {
      ...context,
      stack: stack || 'No stack trace available',
      userAgent: navigator.userAgent,
      url: window.location.href
    }
    
    this.buffer.push({
      level: LogLevel.ERROR,
      message: logMessage,
      context: JSON.stringify(enhancedContext)
    })
    
    // 在控制台中使用带样式的输出，便于识别前端日志
    console.error(`🔴 [FRONTEND ERROR] ${logMessage}`, enhancedContext)
    
    // 错误日志立即刷新
    this.flushImmediately()
  }

  warn(message: string, context?: LogContext) {
    const timestamp = new Date().toISOString()
    const logMessage = `[${timestamp}] ${message}`
    this.buffer.push({
      level: LogLevel.WARN,
      message: logMessage,
      context: context ? JSON.stringify(context) : undefined
    })
    
    // 在控制台中使用带样式的输出，便于识别前端日志
    console.warn(`🟡 [FRONTEND WARN] ${logMessage}`, context)
    
    // 警告日志立即刷新
    this.flushImmediately()
  }

  info(message: string, context?: LogContext) {
    const timestamp = new Date().toISOString()
    const logMessage = `[${timestamp}] ${message}`
    this.buffer.push({
      level: LogLevel.INFO,
      message: logMessage,
      context: context ? JSON.stringify(context) : undefined
    })
    
    // 在控制台中使用带样式的输出，便于识别前端日志
    console.info(`🔵 [FRONTEND INFO] ${logMessage}`, context)
    
    // 如果缓冲区太大，立即刷新
    if (this.buffer.length >= 5) {
      this.flushImmediately()
    }
  }

  debug(message: string, context?: LogContext) {
    const timestamp = new Date().toISOString()
    const logMessage = `[${timestamp}] ${message}`
    this.buffer.push({
      level: LogLevel.DEBUG,
      message: logMessage,
      context: context ? JSON.stringify(context) : undefined
    })
    
    // 在控制台中使用console.log确保debug日志可见，而不是console.debug
    console.log(`🟢 [FRONTEND DEBUG] ${logMessage}`, context)
    
    // 如果缓冲区太大，立即刷新
    if (this.buffer.length >= 10) {
      this.flushImmediately()
    }
  }

  // 专门用于记录异常的方法
  exception(error: Error, message?: string, context?: LogContext) {
    const timestamp = new Date().toISOString()
    const errorMessage = message || error.message || 'Unknown error'
    const logMessage = `[${timestamp}] EXCEPTION: ${errorMessage}`
    
    const enhancedContext = {
      ...context,
      errorName: error.name,
      errorMessage: error.message,
      stack: error.stack || 'No stack trace available',
      userAgent: navigator.userAgent,
      url: window.location.href,
      timestamp: timestamp
    }
    
    this.buffer.push({
      level: LogLevel.ERROR,
      message: logMessage,
      context: JSON.stringify(enhancedContext)
    })
    
    // 在控制台中使用带样式的输出，便于识别前端日志
    console.error(`🔥 [FRONTEND EXCEPTION] ${logMessage}`, enhancedContext)
    
    // 异常日志立即刷新
    this.flushImmediately()
  }
}

// 全局logger实例
let globalLogger: Logger | null = null

export function useLogger() {
  if (!globalLogger) {
    globalLogger = new Logger()
  }
  return globalLogger
}

// 便捷导出
export const logger = {
  error: (message: string, context?: LogContext) => useLogger().error(message, context),
  warn: (message: string, context?: LogContext) => useLogger().warn(message, context),
  info: (message: string, context?: LogContext) => useLogger().info(message, context),
  debug: (message: string, context?: LogContext) => useLogger().debug(message, context),
  exception: (error: Error, message?: string, context?: LogContext) => useLogger().exception(error, message, context),
  
  // 添加手动刷新和状态检查方法
  flush: async () => {
    const logger = useLogger()
    await (logger as any).flush()
  },
  getBufferSize: () => {
    const logger = useLogger()
    return (logger as any).buffer.length
  },
  getBufferContent: () => {
    const logger = useLogger()
    return [...(logger as any).buffer]
  }
} 