// 浏览器类型程序不得继承 Vite Node 配置使用的全局声明。
// @ts-expect-error 浏览器代码中不应存在 Node.js 的 process 全局变量。
export type BrowserProcessMustBeUnavailable = typeof process

// @ts-expect-error 浏览器代码中不应存在 Node.js 的 Buffer 全局变量。
export type BrowserBufferMustBeUnavailable = typeof Buffer
