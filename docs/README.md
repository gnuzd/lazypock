# lazypock-docs

Trang tài liệu (docs) kiểu PocketBase cho SDK **lazypock-ts**, build bằng SvelteKit + Tailwind CSS v4.
Theme màu dùng đúng bộ CSS variables `wabisabi-light` (kiểu daisyUI 5) mà bạn cung cấp — định nghĩa trong
`src/app.css`, dùng chung cho cả Tailwind `@theme` (để sinh utilities `bg-base-100`, `text-primary-content`, `rounded-box`, ...)
và block `:root` / `[data-theme='wabisabi-light']` để runtime-switchable sau này (chỉ cần thêm theme khác, vd `wabisabi-dark`).

## Chạy thử

```bash
npm install
npm run dev
```

Mở http://localhost:5173

## Build

```bash
npm run build
npm run preview
```

## Cấu trúc

```
src/
  app.css                    # Tailwind v4 import + theme tokens wabisabi-light
  app.html                   # HTML shell, đặt sẵn data-theme="wabisabi-light"
  routes/
    +layout.svelte           # Navbar + sidebar (giống bố cục docs PocketBase)
    +page.svelte             # Toàn bộ nội dung docs (single-page, section theo id)
  lib/
    nav.ts                   # Danh sách mục lục sidebar
    components/
      CodeBlock.svelte       # Component hiển thị code block
```

## Thêm theme khác

Copy nguyên khối `:root, [data-theme='wabisabi-light'] { ... }` trong `app.css`,
đổi selector thành `[data-theme='wabisabi-dark']` và đổi giá trị oklch, rồi set
`data-theme="wabisabi-dark"` trên `<html>` hoặc qua `input.theme-controller` (kiểu daisyUI)
để chuyển theme lúc runtime — không cần đổi lại các class Tailwind đã dùng.

## Nội dung tham khảo

Nội dung API/Quick Start lấy theo README của
[lazypock-ts](https://github.com/gnuzd/lazypock-ts) — TypeScript SDK cho
[lazypock](https://github.com/gnuzd/lazypock) (backend tương thích PocketBase).
