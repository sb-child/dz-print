#set page(width: 48mm, height: auto, margin: (top: 5mm, bottom: 5mm, left: 0mm, right: 0mm))
#set text(font: "Unifont", size: 8pt)

= 测试页
- 你好喵，这里是色妹妹。欢迎使用我的项目喵～
- https://github.com/sb-child/dz-print
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
- 目前仅内置 Unifont 字体，不支持渲染数学公式
  - 推荐设置 `#set text(font: "Unifont", size: 8pt)`
- 目前不支持导入 Typst 文件以及其他资源
- 渲染为宽度 576px 的图片后，会经过亮度截断处理为黑白位图
- 在考虑做二值化了
= 字体样式
这个字体好像不支持样式，示例：
- \#strike[\...]：#strike[删除线喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM]
- \#underline[\...]：#underline[下划线喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM]
- 加粗：*加粗喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM*
- 斜体：_斜体喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM_
- 代码块：`代码块喵1234567890aqwsedrftgyhujikolpzxcvbnmQWERTYUIOPASDFGHJKLZXCVBNM`
