/// <reference types="vite/client" />

// 引入 vite/client 后，`import qr from "../assets/support-qrcode.png"`
// 这类资源导入自带 `string` 类型（vite/client 已声明 `*.png` 等模块）。
// 这里只需提供引用，不要再重复声明一遍 `*.png`。
