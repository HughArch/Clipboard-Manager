/**
 * 应用版本配置
 * 这是应用版本的唯一定义源，所有前端组件都应该从这里引用版本信息
 */

export const APP_VERSION = '1.1.7';

/**
 * 获取应用版本
 * @returns 应用版本字符串
 */
export const getAppVersion = (): string => {
  return APP_VERSION;
};

/**
 * 获取格式化的版本信息
 * @returns 带有 'v' 前缀的版本字符串
 */
export const getFormattedVersion = (): string => {
  return `v${APP_VERSION}`;
};