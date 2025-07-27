#set page(width: 48mm, height: auto, margin: (top: 5mm, bottom: 5mm, left: 0mm, right: 0mm))
#set text(font: "Unifont", size: 8pt)
#show math.equation: set text(font: "UnifontExMono")
#let ufex(body) = {
  set text(font: "UnifontExMono")
  [- #body]
}

= 测试页
- 你好喵，这里是色妹妹。欢迎使用我的项目喵～
- https://github.com/sb-child/dz-print
== btw
- 由于 SDK 代码并不开源，本驱动的底层开发完全基于对官方 SDK 的逆向、抓包、说明文档和上机测试。
- 官方 SDK 简直太烂了\... 错别字先不提, 代码里不知道写了多少封装类, 然后 PC 版重要的功能也 todo 了, Android 版倒是 2024 年还有更新\...
= 57mm 小票纸示例
- 打印头参数
  - 宽度: 48mm
  - 像素数: 576
- 打印纸参数
  - 宽度: 57mm
- 打印机型号: DP27P
= Typst 要求
- 高度和页数不限，宽度必须等于 48mm
  - 推荐设置 `#set page(width: 48mm, height: auto, margin: (top: 5mm, bottom: 5mm, left: 0mm, right: 0mm))`
- 目前仅内置 Unifont 和 UnifontExMono 字体，其中 UnifontExMono 字体支持渲染数学公式
  - 推荐设置 `#set text(font: "Unifont", size: 8pt)`
  - 还有 `#show math.equation: set text(font: "UnifontExMono")`
  - 或者 `#set text(font: "UnifontExMono", size: 8pt)`
- 目前不支持导入 Typst 文件以及其他资源，目前的行为是 return AccessDenied
- 渲染为宽度 576px 的图片后，会经过亮度截断处理为黑白位图，在考虑做二值化了，todo
= 字体样式
Unifont 字体没有别的 variant，所以斜体和粗体是不工作的，todo：
- \#strike[\...]：#strike[删除线喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM]
- \#underline[\...]：#underline[下划线喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM]
- 加粗：*加粗喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM*
- 斜体：_斜体喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM_
- 粗斜体：*粗_斜体喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM_*
- 代码块：`代码块喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM`
= 特殊字符
感谢 UnifontExMono 扩展了一堆奇怪的字符，示例：
- LGBT Symbols | #ufex[🜧🜥🜱⚴🜨⚤⚣⚢⮉⛿🜬☌⮋]
- LGBT Symbol2 | #ufex[☿⚧⚥⚨⚦⚩⚲♁🜠🜜⚳⚸⯛]
- LGBT Symbols3 | #ufex[🜐🜻🜮🜢🜫🜭⚪⚬♂♀⚭⚮⚯]
= 数学
感谢 UnifontExMono 也加上了渲染数学符号所需的字符，示例：
- $
    7.32 beta +
    sum_(i=0)^nabla
    (Q_i (a_i - epsilon)) / 2
  $
- The equation $Q = rho A v + C$ defines the glacial flow rate.
- $ Q = rho A v + "time offset" $
- $ v := vec(x_1, x_2, x_3) $
- $ a arrow.squiggly b $
