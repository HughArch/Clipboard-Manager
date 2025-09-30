import { ref, computed } from 'vue'

// 图片缓存配置
const THUMBNAIL_CACHE_SIZE = 200 // 最多缓存200个缩略图
const THUMBNAIL_WIDTH = 200 // 缩略图宽度
const THUMBNAIL_HEIGHT = 150 // 缩略图高度
const THUMBNAIL_QUALITY = 0.7 // 压缩质量

/**
 * LRU缓存管理器
 */
class ThumbnailCache {
  private cache = new Map<string, string>()
  private maxSize: number

  constructor(maxSize = THUMBNAIL_CACHE_SIZE) {
    this.maxSize = maxSize
  }

  get(key: string): string | undefined {
    const value = this.cache.get(key)
    if (value) {
      // 更新LRU顺序
      this.cache.delete(key)
      this.cache.set(key, value)
    }
    return value
  }

  set(key: string, value: string) {
    if (this.cache.has(key)) {
      this.cache.delete(key)
    } else if (this.cache.size >= this.maxSize) {
      // 删除最久未使用的项
      const firstKey = this.cache.keys().next().value
      this.cache.delete(firstKey)
    }
    this.cache.set(key, value)
  }

  clear() {
    this.cache.clear()
  }

  size() {
    return this.cache.size
  }
}

// 全局缓存实例
const thumbnailCache = new ThumbnailCache()

/**
 * 生成图片缩略图
 */
export const generateThumbnail = async (
  base64Data: string, 
  width = THUMBNAIL_WIDTH, 
  height = THUMBNAIL_HEIGHT,
  quality = THUMBNAIL_QUALITY
): Promise<string> => {
  return new Promise((resolve, reject) => {
    const canvas = document.createElement('canvas')
    const ctx = canvas.getContext('2d')
    const img = new Image()
    
    if (!ctx) {
      reject(new Error('无法获取Canvas上下文'))
      return
    }
    
    img.onload = () => {
      try {
        // 计算等比例缩放尺寸
        const aspectRatio = img.width / img.height
        let targetWidth = width
        let targetHeight = height
        
        if (aspectRatio > width / height) {
          // 图片更宽，以宽度为准
          targetHeight = width / aspectRatio
        } else {
          // 图片更高，以高度为准
          targetWidth = height * aspectRatio
        }
        
        canvas.width = targetWidth
        canvas.height = targetHeight
        
        // 绘制缩略图
        ctx.drawImage(img, 0, 0, targetWidth, targetHeight)
        
        // 导出为JPEG格式以减小文件大小
        const thumbnailBase64 = canvas.toDataURL('image/jpeg', quality)
        resolve(thumbnailBase64)
      } catch (error) {
        reject(error)
      }
    }
    
    img.onerror = () => {
      reject(new Error('图片加载失败'))
    }
    
    img.src = base64Data
  })
}

/**
 * 图片缓存Hook
 */
export const useImageCache = () => {
  const isGenerating = ref(false)
  
  /**
   * 获取缩略图，支持缓存
   */
  const getThumbnail = async (itemId: string, originalBase64: string): Promise<string> => {
    // 先从缓存中查找
    const cached = thumbnailCache.get(itemId)
    if (cached) {
      return cached
    }
    
    // 生成新的缩略图
    try {
      isGenerating.value = true
      const thumbnail = await generateThumbnail(originalBase64)
      
      // 存入缓存
      thumbnailCache.set(itemId, thumbnail)
      
      return thumbnail
    } catch (error) {
      console.error('生成缩略图失败:', error)
      // 如果生成失败，返回原图
      return originalBase64
    } finally {
      isGenerating.value = false
    }
  }
  
  /**
   * 预生成缩略图（用于后台批量处理）
   */
  const preGenerateThumbnail = async (itemId: string, originalBase64: string): Promise<void> => {
    if (!thumbnailCache.get(itemId)) {
      try {
        const thumbnail = await generateThumbnail(originalBase64)
        thumbnailCache.set(itemId, thumbnail)
      } catch (error) {
        console.error('预生成缩略图失败:', error)
      }
    }
  }
  
  /**
   * 清理缓存
   */
  const clearCache = () => {
    thumbnailCache.clear()
  }
  
  /**
   * 获取缓存状态
   */
  const cacheStatus = computed(() => ({
    size: thumbnailCache.size(),
    maxSize: THUMBNAIL_CACHE_SIZE,
    usage: (thumbnailCache.size() / THUMBNAIL_CACHE_SIZE * 100).toFixed(1) + '%'
  }))
  
  return {
    getThumbnail,
    preGenerateThumbnail,
    clearCache,
    isGenerating,
    cacheStatus
  }
}

/**
 * 图片懒加载Hook
 */
export const useImageLazyLoad = () => {
  const imageObserver = ref<IntersectionObserver | null>(null)
  const loadedImages = ref(new Set<string>())
  
  /**
   * 初始化Intersection Observer
   */
  const initObserver = () => {
    if (imageObserver.value) return
    
    imageObserver.value = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            const img = entry.target as HTMLImageElement
            const dataSrc = img.dataset.src
            
            if (dataSrc && !loadedImages.value.has(dataSrc)) {
              img.src = dataSrc
              img.removeAttribute('data-src')
              loadedImages.value.add(dataSrc)
              imageObserver.value?.unobserve(img)
            }
          }
        })
      },
      {
        rootMargin: '50px', // 提前50px开始加载
        threshold: 0.1
      }
    )
  }
  
  /**
   * 观察图片元素
   */
  const observeImage = (element: HTMLImageElement) => {
    if (!imageObserver.value) {
      initObserver()
    }
    imageObserver.value?.observe(element)
  }
  
  /**
   * 停止观察
   */
  const unobserveImage = (element: HTMLImageElement) => {
    imageObserver.value?.unobserve(element)
  }
  
  /**
   * 销毁观察器
   */
  const destroyObserver = () => {
    if (imageObserver.value) {
      imageObserver.value.disconnect()
      imageObserver.value = null
    }
    loadedImages.value.clear()
  }
  
  return {
    observeImage,
    unobserveImage,
    destroyObserver,
    loadedImages
  }
}
