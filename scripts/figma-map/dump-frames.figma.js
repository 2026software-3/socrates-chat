// 唯讀：列出一個 Figma 頁面的區塊（SECTION）與畫面 frame，輸出給 build.mjs 讀的文字格式。
// 用法：把 PAGE_ID 換成要讀的頁面，整段貼進 Figma MCP 的 use_figma（只讀，不修改檔案），
// 回傳的文字原樣存成 scripts/figma-map/frames/<頁面>.txt。格式見 README.md。
const PAGE_ID = '21:241'
const page = await figma.getNodeByIdAsync(PAGE_ID)
await figma.setCurrentPageAsync(page)
const line = (n, indent) => `${indent}${n.type === 'SECTION' ? 'S' : 'F'} ${n.id} ${Math.round(n.width)}x${Math.round(n.height)} ${n.name}`
const isScreen = (n) => n.type === 'FRAME' && n.width >= 300 // 排除 100x100 之類的暫存 frame
const out = [`# ${page.name} (${page.id})`]
for (const n of page.children) {
  if (n.type === 'SECTION') {
    out.push(line(n, ''))
    if (n.name.startsWith('Overlays')) continue // 帳號選單、平面圖面板等覆蓋層：只記區塊
    for (const c of n.children) if (isScreen(c)) out.push(line(c, '  '))
  } else if (isScreen(n)) {
    out.push(line(n, ''))
  }
}
return out.join('\n')
